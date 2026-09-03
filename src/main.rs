use std::io::{self, IsTerminal, Read};

use clap::{CommandFactory, Parser};
use serde::Serialize;
use serde_json::Value;

use outlook_cli::auth;
use outlook_cli::cli::{
    AuthCommand, CalendarCommand, Cli, Command, ConfigCommand, InitArgs, MailCommand, PageArgs,
};
use outlook_cli::config::{self, Profile};
use outlook_cli::error::AppError;
use outlook_cli::graph::{self, GraphClient, Page};
use outlook_cli::output::{Output, OutputFormat, print_error, structured_from_args};
use outlook_cli::schema;

#[tokio::main]
async fn main() {
    let structured = structured_from_args();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                error.exit();
            }
            if structured {
                let error = AppError::InvalidInput(error.to_string());
                print_error(&error, true);
                std::process::exit(error.contract().exit_code);
            }
            error.exit();
        }
    };
    let out = Output {
        format: if cli.json && cli.output == OutputFormat::Auto {
            OutputFormat::Json
        } else {
            cli.output
        },
        quiet: cli.quiet,
    };
    if let Err(error) = dispatch(cli, out).await {
        print_error(&error, out.json());
        std::process::exit(error.contract().exit_code);
    }
}

async fn dispatch(cli: Cli, out: Output) -> Result<(), AppError> {
    let profile = cli.profile.as_deref();
    let command = cli.command.ok_or_else(|| {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            AppError::InvalidInput("no command was provided; run `outlook --help` or `outlook init --client-id APPLICATION_ID`".into())
        } else {
            AppError::NonInteractive(
                "no command was provided; run `outlook --help` or `outlook schema`".into(),
            )
        }
    })?;
    match command {
        Command::Init(args) => init(profile.unwrap_or("default"), args, out).await,
        Command::Auth { command } => auth_command(profile, command, out).await,
        Command::Config { command } => config_command(profile, command, out),
        Command::Whoami => {
            let value = client(profile).await?.me().await?;
            out.value(&value, || identity_text(&value))
        }
        Command::Inbox(page) => list_messages(profile, "inbox", page, out).await,
        Command::Mail {
            command: MailCommand::List { folder, page },
        } => list_messages(profile, &folder, page, out).await,
        Command::Mail {
            command: MailCommand::Read { id },
        } => {
            let value = client(profile).await?.message(&id).await?;
            out.value(&value, || message_text(&value))
        }
        Command::Mail {
            command:
                MailCommand::Send {
                    to,
                    cc,
                    subject,
                    body,
                },
        } => {
            validate_addresses(&to)?;
            validate_addresses(&cc)?;
            if subject.trim().is_empty() {
                return Err(AppError::InvalidInput("subject cannot be empty".into()));
            }
            let body = read_body(&body)?;
            let graph = writable_client(profile).await?;
            let value = graph.send_mail(&to, &cc, &subject, &body).await?;
            out.value(&value, || format!("Sent “{subject}” to {}", to.join(", ")))
        }
        Command::Mail {
            command: MailCommand::Reply { id, body, all },
        } => {
            let body = read_body(&body)?;
            if body.trim().is_empty() {
                return Err(AppError::InvalidInput("reply body cannot be empty".into()));
            }
            let value = writable_client(profile)
                .await?
                .reply(&id, &body, all)
                .await?;
            out.value(&value, || {
                if all {
                    "Reply-all sent.".into()
                } else {
                    "Reply sent.".into()
                }
            })
        }
        Command::Mail {
            command: MailCommand::Move { id, destination },
        } => {
            if destination.trim().is_empty() {
                return Err(AppError::InvalidInput("destination cannot be empty".into()));
            }
            let value = writable_client(profile)
                .await?
                .move_message(&id, &destination)
                .await?;
            out.value(&value, || format!("Moved message to {destination}."))
        }
        Command::Calendar {
            command:
                CalendarCommand::Agenda {
                    start,
                    end,
                    timezone,
                    page,
                },
        } => {
            validate_range(&start, &end)?;
            let mut result = client(profile)
                .await?
                .agenda(&start, &end, &timezone, page.limit, page.cursor.as_deref())
                .await?;
            graph::select_fields(&mut result, page.fields.as_deref())?;
            render_page(&result, out, event_line)
        }
        Command::Calendar {
            command:
                CalendarCommand::Create {
                    subject,
                    start,
                    end,
                    timezone,
                    attendee,
                    body,
                },
        } => {
            validate_range(&start, &end)?;
            validate_addresses(&attendee)?;
            if subject.trim().is_empty() {
                return Err(AppError::InvalidInput("subject cannot be empty".into()));
            }
            let value = writable_client(profile)
                .await?
                .create_event(&subject, &start, &end, &timezone, &attendee, &body)
                .await?;
            out.value(&value, || format!("Created “{subject}”."))
        }
        Command::Doctor { offline } => doctor(profile, offline, out).await,
        Command::Capabilities => capabilities(out),
        Command::Schema { command } => {
            let value = schema::generate(command.as_deref());
            if let Some(name) = command
                && value["commands"].as_array().is_some_and(Vec::is_empty)
            {
                return Err(AppError::NotFound(format!(
                    "command '{}' is not declared",
                    name
                )));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&value)
                    .map_err(|error| AppError::Unexpected(error.to_string()))?
            );
            Ok(())
        }
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "outlook", &mut io::stdout());
            Ok(())
        }
    }
}

async fn init(profile_name: &str, args: InitArgs, out: Output) -> Result<(), AppError> {
    let client_id = args.client_id.filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::InvalidInput("a public-client application ID is required; pass --client-id or set OUTLOOK_CLIENT_ID".into()))?;
    if args.tenant.trim().is_empty() {
        return Err(AppError::InvalidInput("tenant cannot be empty".into()));
    }
    let profile = Profile {
        client_id,
        tenant: args.tenant,
        read_only: args.read_only,
    };
    let path = config::save(profile_name, profile.clone())?;
    let token = if args.no_login {
        None
    } else {
        Some(auth::login(profile_name, &profile, &out).await?)
    };
    #[derive(Serialize)]
    struct Result<'a> {
        profile: &'a str,
        config_path: String,
        signed_in: bool,
        tenant: &'a str,
        read_only: bool,
    }
    let result = Result {
        profile: profile_name,
        config_path: path.display().to_string(),
        signed_in: token.is_some(),
        tenant: &profile.tenant,
        read_only: profile.read_only,
    };
    out.value(&result, || {
        if token.is_some() {
            "You’re ready. Run `outlook inbox`.".into()
        } else {
            format!("Saved {}. Next: `outlook auth login`", path.display())
        }
    })
}

async fn auth_command(
    profile_arg: Option<&str>,
    command: AuthCommand,
    out: Output,
) -> Result<(), AppError> {
    match command {
        AuthCommand::Login => {
            let (name, profile) = config::load(profile_arg)?;
            let token = auth::login(&name, &profile, &out).await?;
            let value = serde_json::json!({"profile":name,"expires_at":token.expires_at,"scope":token.scope});
            out.value(&value, || format!("Signed in profile '{name}'."))
        }
        AuthCommand::Logout => {
            let (name, _) = config::load(profile_arg)?;
            auth::logout(&name)?;
            let value = serde_json::json!({"profile":name,"signed_in":false});
            out.value(&value, || format!("Signed out profile '{name}'."))
        }
        AuthCommand::Status => {
            let configured = config::configured_profile(profile_arg);
            let name = configured
                .as_ref()
                .map(|(name, _)| name.as_str())
                .unwrap_or(profile_arg.unwrap_or("default"));
            let value = serde_json::json!({
                "profile":name,"configured":configured.is_some(),"signed_in":auth::has_token(name),
                "read_only":configured.as_ref().is_some_and(|(_, profile)| profile.read_only),
                "granted_scopes":auth::granted_scopes(name),"config_path":config::path()
            });
            out.value(&value, || {
                format!(
                    "Profile: {name}\nConfigured: {}\nSigned in: {}\nRead only: {}\nConfig: {}",
                    yes_no(configured.is_some()),
                    yes_no(auth::has_token(name)),
                    yes_no(value["read_only"].as_bool().unwrap_or(false)),
                    config::path().display()
                )
            })
        }
    }
}

fn config_command(
    profile_arg: Option<&str>,
    command: ConfigCommand,
    out: Output,
) -> Result<(), AppError> {
    match command {
        ConfigCommand::Path => out.value(&config::path().display().to_string(), || {
            config::path().display().to_string()
        }),
        ConfigCommand::Show => {
            let (name, profile) = config::load(profile_arg)?;
            let value = serde_json::json!({"profile":name,"client_id":profile.client_id,"tenant":profile.tenant,"read_only":profile.read_only,"config_path":config::path()});
            out.value(&value, || {
                format!(
                    "Profile: {name}\nTenant: {}\nClient ID: {}\nRead only: {}\nConfig: {}",
                    profile.tenant,
                    profile.client_id,
                    yes_no(profile.read_only),
                    config::path().display()
                )
            })
        }
    }
}

async fn client(profile_arg: Option<&str>) -> Result<GraphClient, AppError> {
    let (name, profile) = config::load(profile_arg)?;
    Ok(GraphClient::new(auth::access_token(&name, &profile).await?))
}

async fn writable_client(profile_arg: Option<&str>) -> Result<GraphClient, AppError> {
    let (name, profile) = config::load(profile_arg)?;
    profile.require_writable()?;
    Ok(GraphClient::new(auth::access_token(&name, &profile).await?))
}

async fn list_messages(
    profile: Option<&str>,
    folder: &str,
    page: PageArgs,
    out: Output,
) -> Result<(), AppError> {
    let mut result = client(profile)
        .await?
        .messages(folder, page.limit, page.cursor.as_deref())
        .await?;
    graph::select_fields(&mut result, page.fields.as_deref())?;
    render_page(&result, out, message_line)
}

fn render_page(page: &Page, out: Output, line: fn(&Value) -> String) -> Result<(), AppError> {
    out.value(page, || {
        let mut text = if page.items.is_empty() {
            "No results.".into()
        } else {
            page.items.iter().map(line).collect::<Vec<_>>().join("\n")
        };
        if page.truncated {
            text.push_str(
                "\n\nMore results are available; use --cursor with next_cursor from JSON output.",
            );
        }
        text
    })
}

async fn doctor(profile_arg: Option<&str>, offline: bool, out: Output) -> Result<(), AppError> {
    let configured = config::configured_profile(profile_arg);
    let name = configured
        .as_ref()
        .map(|(name, _)| name.as_str())
        .unwrap_or(profile_arg.unwrap_or("default"));
    let signed_in = auth::has_token(name);
    let mut checks = vec![
        serde_json::json!({"name":"configuration","ok":configured.is_some(),"detail":if configured.is_some(){"profile is configured"}else{"run outlook init --client-id APPLICATION_ID"}}),
        serde_json::json!({"name":"credentials","ok":signed_in,"detail":if signed_in{"credential is available"}else{"run outlook auth login"}}),
    ];
    if !offline && configured.is_some() && signed_in {
        match client(profile_arg).await?.me().await {
            Ok(identity) => checks.push(serde_json::json!({"name":"microsoft_graph","ok":true,"detail":identity.pointer("/userPrincipalName").and_then(Value::as_str)})),
            Err(error) => checks.push(serde_json::json!({"name":"microsoft_graph","ok":false,"detail":error.to_string()})),
        }
    }
    let healthy = checks.iter().all(|check| check["ok"] == true);
    let value = serde_json::json!({"checks":checks,"healthy":healthy,"offline":offline});
    out.value(&value, || {
        checks
            .iter()
            .map(|check| {
                format!(
                    "{} {} — {}",
                    if check["ok"] == true { "✓" } else { "✗" },
                    check["name"].as_str().unwrap_or("check"),
                    check["detail"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn capabilities(out: Output) -> Result<(), AppError> {
    let value = serde_json::json!({
        "supported":["delegated device-code OAuth","personal and work/school accounts","inbox and folder listing","message reading, sending, replying, and moving","calendar agenda and event creation","immutable Outlook IDs","read-only profiles","CLI Spec v0.3"],
        "planned":["browser PKCE login","keyboard-first TUI","drafts and attachments","mail search","meeting responses","contacts and categories","delta synchronization and local cache"],
        "api":"Microsoft Graph v1.0"
    });
    out.value(&value, || "Supported: mail and calendar essentials, safe automation contracts, and device-code OAuth.\nPlanned: TUI, drafts, attachments, search, responses, contacts, and delta sync.".into())
}

fn read_body(raw: &str) -> Result<String, AppError> {
    if raw != "-" {
        return Ok(raw.into());
    }
    let mut body = String::new();
    io::stdin().read_to_string(&mut body)?;
    Ok(body)
}

fn validate_addresses(addresses: &[String]) -> Result<(), AppError> {
    if let Some(address) = addresses
        .iter()
        .find(|address| !address.contains('@') || address.trim().contains(char::is_whitespace))
    {
        return Err(AppError::InvalidInput(format!(
            "invalid email address: {address}"
        )));
    }
    Ok(())
}

fn validate_range(start: &str, end: &str) -> Result<(), AppError> {
    if start.trim().is_empty() || end.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "start and end cannot be empty".into(),
        ));
    }
    if start == end {
        return Err(AppError::InvalidInput(
            "start and end cannot be equal".into(),
        ));
    }
    Ok(())
}

fn identity_text(value: &Value) -> String {
    format!(
        "{}\n{}",
        string(value, "/displayName"),
        value
            .pointer("/mail")
            .or_else(|| value.pointer("/userPrincipalName"))
            .and_then(Value::as_str)
            .unwrap_or("")
    )
}

fn message_text(value: &Value) -> String {
    let from = value
        .pointer("/from/emailAddress/address")
        .and_then(Value::as_str)
        .unwrap_or("unknown sender");
    let body = value
        .pointer("/body/content")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/bodyPreview").and_then(Value::as_str))
        .unwrap_or("");
    format!(
        "{}\nFrom: {from}\nDate: {}\n\n{body}",
        string(value, "/subject"),
        string(value, "/receivedDateTime")
    )
}

fn message_line(value: &Value) -> String {
    let unread = if value
        .pointer("/isRead")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        " "
    } else {
        "●"
    };
    let from = value
        .pointer("/from/emailAddress/name")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/from/emailAddress/address")
                .and_then(Value::as_str)
        })
        .unwrap_or("unknown");
    format!(
        "{unread} {:<24}  {:<42}  {}",
        truncate(from, 24),
        truncate(string(value, "/subject"), 42),
        string(value, "/receivedDateTime")
    )
}

fn event_line(value: &Value) -> String {
    format!(
        "{}–{}  {}",
        string(value, "/start/dateTime"),
        string(value, "/end/dateTime"),
        string(value, "/subject")
    )
}

fn string<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}
fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        value.into()
    } else {
        value
            .chars()
            .take(width.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}
fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
