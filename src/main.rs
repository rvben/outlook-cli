use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;

use clap::{CommandFactory, FromArgMatches};
use serde::Serialize;
use serde_json::Value;

use outlook_cli::auth;
use outlook_cli::backend::MailBackend;
use outlook_cli::cli::{
    AttachmentCommand, AuthCommand, CalendarCommand, Cli, Command, ConfigCommand, DraftCommand,
    InitArgs, MailCommand, PageArgs, ProfileCommand,
};
use outlook_cli::config::{self, BackendKind, Profile};
use outlook_cli::desktop::{self, DesktopClient};
use outlook_cli::error::AppError;
use outlook_cli::graph::{self, GraphClient, Page};
use outlook_cli::output::{Output, OutputFormat, print_error, structured_from_args};
use outlook_cli::presentation::{PageKind, message_text, page_text};
use outlook_cli::schema;

#[tokio::main]
async fn main() {
    let structured = structured_from_args();
    let no_color = std::env::args()
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--no-color")
        || std::env::var_os("NO_COLOR").is_some();
    outlook_cli::output::set_no_color(no_color);
    let command = if no_color {
        Cli::command().color(clap::ColorChoice::Never)
    } else {
        Cli::command()
    };
    let cli = match command
        .try_get_matches()
        .and_then(|matches| Cli::from_arg_matches(&matches))
    {
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
    if cli.command.is_none()
        && cli.output != OutputFormat::Json
        && !cli.json
        && io::stdin().is_terminal()
        && io::stdout().is_terminal()
    {
        let mut help = Cli::command();
        if no_color {
            help = help.color(clap::ColorChoice::Never);
        }
        let _ = help.print_help();
        println!();
        return;
    }
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
    let yes = cli.yes;
    let command = cli.command.ok_or_else(|| {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            AppError::InvalidInput(
                "no command was provided; run `outlook --help` or `outlook init`".into(),
            )
        } else {
            AppError::NonInteractive(
                "no command was provided; run `outlook --help` or `outlook schema`".into(),
            )
        }
    })?;
    if let Some((_, selected)) = config::configured_profile(profile)
        && selected.backend == BackendKind::Desktop
    {
        require_desktop_command(&command, &selected)?;
    }
    match command {
        Command::Tui {
            folder,
            demo,
            snapshot,
            width,
            height,
        } => {
            if snapshot {
                if folder != "inbox" {
                    return Err(AppError::InvalidInput(
                        "--snapshot shows the demo inbox; omit --folder".into(),
                    ));
                }
                println!("{}", outlook_cli::tui::snapshot(width, height)?);
                Ok(())
            } else {
                if out.format == OutputFormat::Json {
                    return Err(AppError::InvalidInput("`outlook tui` is interactive; use `outlook inbox --output json` for structured data".into()));
                }
                outlook_cli::tui::run(profile, folder, demo, cli.no_color).await
            }
        }
        Command::Init(args) => init(profile.unwrap_or("default"), args, out).await,
        Command::Auth { command } => auth_command(profile, command, out).await,
        Command::Profile { command } => profile_command(command, yes, out),
        Command::Config { command } => config_command(profile, command, out),
        Command::Whoami => {
            let value = client(profile).await?.me().await?;
            out.value(&value, || identity_text(&value))
        }
        Command::Inbox(page) => list_messages(profile, "inbox", page, out).await,
        Command::Mail {
            command: MailCommand::Folders { parent, page },
        } => {
            if parent.as_ref().is_some_and(|p| p.trim().is_empty()) {
                return Err(AppError::InvalidInput(
                    "parent folder cannot be empty".into(),
                ));
            }
            let mut result = MailBackend::connect(profile)
                .await?
                .folders(parent.as_deref(), page.limit, page.cursor.as_deref())
                .await?;
            graph::select_fields(&mut result, page.fields.as_deref())?;
            render_page(&result, out, PageKind::Folders)
        }
        Command::Mail {
            command: MailCommand::List { folder, page },
        } => list_messages(profile, &folder, page, out).await,
        Command::Mail {
            command: MailCommand::Read { id },
        } => {
            let value = MailBackend::connect(profile).await?.message(&id).await?;
            out.value(&value, || message_text(&value))
        }
        Command::Mail {
            command:
                MailCommand::Search {
                    query,
                    folder,
                    page,
                },
        } => {
            if query.trim().is_empty() {
                return Err(AppError::InvalidInput(
                    "search query cannot be empty".into(),
                ));
            }
            if folder
                .as_ref()
                .is_some_and(|folder| folder.trim().is_empty())
            {
                return Err(AppError::InvalidInput("folder cannot be empty".into()));
            }
            let mut result = MailBackend::connect(profile)
                .await?
                .search_messages(
                    &query,
                    folder.as_deref(),
                    page.limit,
                    page.cursor.as_deref(),
                )
                .await?;
            graph::select_fields(&mut result, page.fields.as_deref())?;
            render_page(&result, out, PageKind::Messages)
        }
        Command::Mail {
            command: MailCommand::MarkRead { id },
        } => set_message_read(profile, &id, true, out).await,
        Command::Mail {
            command: MailCommand::MarkUnread { id },
        } => set_message_read(profile, &id, false, out).await,
        Command::Mail {
            command:
                MailCommand::Send {
                    to,
                    cc,
                    bcc,
                    subject,
                    body,
                },
        } => {
            validate_addresses(&to)?;
            validate_addresses(&cc)?;
            validate_addresses(&bcc)?;
            if subject.trim().is_empty() {
                return Err(AppError::InvalidInput("subject cannot be empty".into()));
            }
            let body = read_body(&body)?;
            let backend = MailBackend::connect_writable(profile).await?;
            let value = backend.send_mail(&to, &cc, &bcc, &subject, &body).await?;
            out.value(&value, || format!("Sent “{subject}” to {}", to.join(", ")))
        }
        Command::Mail {
            command: MailCommand::Reply { id, body, all },
        } => {
            let body = read_body(&body)?;
            if body.trim().is_empty() {
                return Err(AppError::InvalidInput("reply body cannot be empty".into()));
            }
            let value = MailBackend::connect_writable(profile)
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
            let value = MailBackend::connect_writable(profile)
                .await?
                .move_message(&id, &destination)
                .await?;
            out.value(&value, || format!("Moved message to {destination}."))
        }
        Command::Mail {
            command: MailCommand::Delete { id },
        } => {
            confirm_destructive(yes, "Delete this message?")?;
            let value = MailBackend::connect_writable(profile)
                .await?
                .delete_message(&id)
                .await?;
            out.value(&value, || "Deleted message.".into())
        }
        Command::Mail {
            command: MailCommand::Draft { command },
        } => draft_command(profile, command, yes, out).await,
        Command::Mail {
            command: MailCommand::Attachment { command },
        } => attachment_command(profile, command, yes, out).await,
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
            render_page(&result, out, PageKind::Events)
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
    if args.backend == BackendKind::Desktop {
        let profile = Profile {
            backend: BackendKind::Desktop,
            client_id: String::new(),
            tenant: String::new(),
            read_only: args.read_only,
        };
        let connection = if args.no_login {
            None
        } else {
            Some(DesktopClient.probe().await?)
        };
        let path = config::save(profile_name, profile)?;
        let value = serde_json::json!({"profile":profile_name,"backend":"desktop","config_path":path,"signed_in":connection.is_some(),"read_only":args.read_only,"client_id":"","tenant":"","connection":connection});
        return out.value(&value, || format!("Configured desktop profile '{profile_name}'. Uses the active classic Outlook profile on Windows.\nConfig: {}", path.display()));
    }
    let client_id = match args.client_id {
        Some(value) if value.trim().is_empty() => {
            return Err(AppError::InvalidInput("client ID cannot be empty".into()));
        }
        Some(value) => value,
        None => config::DEFAULT_CLIENT_ID.into(),
    };
    if args.tenant.trim().is_empty() {
        return Err(AppError::InvalidInput("tenant cannot be empty".into()));
    }
    let profile = Profile {
        backend: BackendKind::Graph,
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
        backend: &'static str,
        profile: &'a str,
        client_id: &'a str,
        config_path: String,
        signed_in: bool,
        tenant: &'a str,
        read_only: bool,
    }
    let result = Result {
        backend: "graph",
        profile: profile_name,
        client_id: &profile.client_id,
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
    if let Some((name, profile)) = config::configured_profile(profile_arg)
        && profile.backend == BackendKind::Desktop
    {
        return match command {
            AuthCommand::Status { offline } => {
                let connection = if offline { None } else { Some(DesktopClient.probe().await?) };
                let value = serde_json::json!({"profile":name,"backend":"desktop","configured":true,"signed_in":connection.as_ref().map(|_| true),"read_only":profile.read_only,"verified":connection.is_some(),"identity":null,"connection":connection,"authentication":"windows_outlook_profile"});
                out.value(&value, || format!("Profile: {name}\nBackend: desktop\nVerified: {}\nAuthentication is managed by classic Outlook on Windows.", yes_no(connection.is_some())))
            }
            _ => Err(AppError::Unsupported("desktop authentication is managed by classic Outlook on Windows; use `outlook auth status` or `outlook doctor` to verify it".into())),
        };
    }
    match command {
        AuthCommand::Login => {
            let (name, profile, initialized) = config::load_or_initialize(profile_arg)?;
            if initialized {
                out.note(format!(
                    "Configured profile '{name}' with the maintained Outlook application."
                ));
            }
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
        AuthCommand::Status { offline } => {
            let configured = config::configured_profile(profile_arg);
            let name = configured
                .as_ref()
                .map(|(name, _)| name.as_str())
                .unwrap_or(profile_arg.unwrap_or("default"));
            let identity = if !offline && configured.is_some() && auth::has_token(name) {
                Some(client(Some(name)).await?.me().await?)
            } else {
                None
            };
            let value = serde_json::json!({
                "profile":name,"configured":configured.is_some(),"signed_in":auth::has_token(name),
                "read_only":configured.as_ref().is_some_and(|(_, profile)| profile.read_only),
                "verified": identity.is_some(), "identity": identity,
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

fn profile_command(command: ProfileCommand, yes: bool, out: Output) -> Result<(), AppError> {
    match command {
        ProfileCommand::List => {
            let profiles = config::profile_summaries()?;
            out.value(
                &serde_json::json!({"items": profiles, "total": profiles.len()}),
                || {
                    if profiles.is_empty() {
                        "No profiles configured. Run `outlook init`.".into()
                    } else {
                        profiles
                            .iter()
                            .map(|profile| {
                                format!(
                                    "{} {}  {}  {}",
                                    if profile.active { "*" } else { " " },
                                    profile.name,
                                    profile.backend.as_str(),
                                    profile.tenant
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                },
            )
        }
        ProfileCommand::Use { name } => {
            config::use_profile(&name)?;
            out.value(
                &serde_json::json!({"profile": name, "active": true}),
                || format!("Active profile set to '{name}'."),
            )
        }
        ProfileCommand::Remove { name } => {
            if !yes {
                return Err(AppError::InvalidInput(
                    "profile removal requires --yes".into(),
                ));
            }
            let (_, selected) = config::load(Some(&name))?;
            if selected.backend == BackendKind::Graph {
                auth::logout(&name)?;
            }
            if !config::remove_profile(&name)? {
                return Err(AppError::InvalidInput(format!(
                    "profile '{name}' is not configured"
                )));
            }
            out.value(
                &serde_json::json!({"profile": name, "removed": true}),
                || format!("Removed profile '{name}'."),
            )
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
            let value = serde_json::json!({"profile":name,"backend":profile.backend,"client_id":profile.client_id,"tenant":profile.tenant,"read_only":profile.read_only,"config_path":config::path()});
            out.value(&value, || {
                format!(
                    "Profile: {name}\nBackend: {}\nTenant: {}\nClient ID: {}\nRead only: {}\nConfig: {}",
                    profile.backend.as_str(),
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
    require_graph(&profile)?;
    Ok(GraphClient::new(auth::access_token(&name, &profile).await?))
}

async fn writable_client(profile_arg: Option<&str>) -> Result<GraphClient, AppError> {
    let (name, profile) = config::load(profile_arg)?;
    profile.require_writable()?;
    require_graph(&profile)?;
    Ok(GraphClient::new(auth::access_token(&name, &profile).await?))
}

async fn list_messages(
    profile: Option<&str>,
    folder: &str,
    page: PageArgs,
    out: Output,
) -> Result<(), AppError> {
    let mut result = MailBackend::connect(profile)
        .await?
        .messages(folder, page.limit, page.cursor.as_deref())
        .await?;
    graph::select_fields(&mut result, page.fields.as_deref())?;
    render_page(&result, out, PageKind::Messages)
}

async fn set_message_read(
    profile: Option<&str>,
    id: &str,
    read: bool,
    out: Output,
) -> Result<(), AppError> {
    let value = MailBackend::connect_writable(profile)
        .await?
        .set_message_read(id, read)
        .await?;
    out.value(&value, || {
        if read {
            "Marked message as read.".into()
        } else {
            "Marked message as unread.".into()
        }
    })
}

async fn draft_command(
    profile: Option<&str>,
    command: DraftCommand,
    yes: bool,
    out: Output,
) -> Result<(), AppError> {
    match command {
        DraftCommand::List(page) => list_messages(profile, "drafts", page, out).await,
        DraftCommand::Create {
            to,
            cc,
            bcc,
            subject,
            body,
        } => {
            validate_addresses(&to)?;
            validate_addresses(&cc)?;
            validate_addresses(&bcc)?;
            let body = read_body(&body)?;
            let value = MailBackend::connect_writable(profile)
                .await?
                .create_draft(&to, &cc, &bcc, &subject, &body)
                .await?;
            out.value(&value, || {
                if subject.is_empty() {
                    "Created untitled draft.".into()
                } else {
                    format!("Created draft “{subject}”.")
                }
            })
        }
        DraftCommand::Update {
            id,
            to,
            clear_to,
            cc,
            clear_cc,
            bcc,
            clear_bcc,
            subject,
            body,
        } => {
            validate_addresses(&to)?;
            validate_addresses(&cc)?;
            validate_addresses(&bcc)?;
            let to = recipient_update(&to, clear_to);
            let cc = recipient_update(&cc, clear_cc);
            let bcc = recipient_update(&bcc, clear_bcc);
            let body = body.as_deref().map(read_body).transpose()?;
            if to.is_none() && cc.is_none() && bcc.is_none() && subject.is_none() && body.is_none()
            {
                return Err(AppError::InvalidInput(
                    "draft update requires at least one field".into(),
                ));
            }
            let value = MailBackend::connect_writable(profile)
                .await?
                .update_draft(&id, to, cc, bcc, subject.as_deref(), body.as_deref())
                .await?;
            out.value(&value, || "Updated draft.".into())
        }
        DraftCommand::Send { id } => {
            let value = MailBackend::connect_writable(profile)
                .await?
                .send_draft(&id)
                .await?;
            out.value(&value, || "Sent draft.".into())
        }
        DraftCommand::Delete { id } => {
            confirm_destructive(yes, "Delete this draft?")?;
            let value = MailBackend::connect_writable(profile)
                .await?
                .delete_draft(&id)
                .await?;
            out.value(&value, || "Deleted draft.".into())
        }
    }
}

fn recipient_update(addresses: &[String], clear: bool) -> Option<&[String]> {
    if clear || !addresses.is_empty() {
        Some(addresses)
    } else {
        None
    }
}

async fn attachment_command(
    profile: Option<&str>,
    command: AttachmentCommand,
    yes: bool,
    out: Output,
) -> Result<(), AppError> {
    match command {
        AttachmentCommand::List { message_id, page } => {
            let mut result = client(profile)
                .await?
                .attachments(&message_id, page.limit, page.cursor.as_deref())
                .await?;
            graph::select_fields(&mut result, page.fields.as_deref())?;
            render_page(&result, out, PageKind::Attachments)
        }
        AttachmentCommand::Add {
            message_id,
            path,
            content_type,
        } => {
            let metadata = std::fs::metadata(&path).map_err(|error| {
                AppError::InvalidInput(format!("cannot read {}: {error}", path.display()))
            })?;
            if !metadata.is_file() {
                return Err(AppError::InvalidInput(format!(
                    "attachment is not a file: {}",
                    path.display()
                )));
            }
            if metadata.len() > graph::MAX_ATTACHMENT_SIZE as u64 {
                return Err(AppError::InvalidInput(
                    "attachments cannot exceed 150 MiB".into(),
                ));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    AppError::InvalidInput("attachment needs a valid file name".into())
                })?;
            let content_type = content_type
                .as_deref()
                .unwrap_or("application/octet-stream");
            if content_type.trim().is_empty() || !content_type.contains('/') {
                return Err(AppError::InvalidInput(
                    "content type must look like type/subtype".into(),
                ));
            }
            let bytes = std::fs::read(&path)?;
            let value = writable_client(profile)
                .await?
                .add_file_attachment(&message_id, name, content_type, &bytes)
                .await?;
            out.value(&value, || format!("Attached {name}."))
        }
        AttachmentCommand::Download {
            message_id,
            attachment_id,
            path,
            force,
        } => {
            validate_download_path(&path, force)?;
            let bytes = client(profile)
                .await?
                .download_attachment(&message_id, &attachment_id)
                .await?;
            write_download(&path, &bytes, force)?;
            let value = serde_json::json!({
                "message_id":message_id,
                "attachment_id":attachment_id,
                "path":path,
                "size":bytes.len()
            });
            out.value(&value, || {
                format!("Downloaded {} bytes to {}.", bytes.len(), path.display())
            })
        }
        AttachmentCommand::Delete {
            message_id,
            attachment_id,
        } => {
            confirm_destructive(yes, "Delete this attachment?")?;
            let value = writable_client(profile)
                .await?
                .delete_attachment(&message_id, &attachment_id)
                .await?;
            out.value(&value, || "Deleted attachment.".into())
        }
    }
}

fn validate_download_path(path: &Path, force: bool) -> Result<(), AppError> {
    if path.exists() && !force {
        return Err(AppError::InvalidInput(format!(
            "destination already exists: {}; pass --force to replace it",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "destination directory does not exist: {}",
            parent.display()
        )));
    }
    Ok(())
}

fn write_download(path: &Path, bytes: &[u8], force: bool) -> Result<(), AppError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".outlook-download-")
        .tempfile_in(parent)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    if force {
        temp.persist(path).map_err(|error| error.error)?;
    } else {
        temp.persist_noclobber(path).map_err(|error| {
            if error.error.kind() == io::ErrorKind::AlreadyExists {
                AppError::InvalidInput(format!(
                    "destination already exists: {}; pass --force to replace it",
                    path.display()
                ))
            } else {
                AppError::Io(error.error)
            }
        })?;
    }
    Ok(())
}

fn render_page(page: &Page, out: Output, kind: PageKind) -> Result<(), AppError> {
    out.value(page, || page_text(page, kind))
}

async fn doctor(profile_arg: Option<&str>, offline: bool, out: Output) -> Result<(), AppError> {
    if let Some((name, profile)) = config::configured_profile(profile_arg)
        && profile.backend == BackendKind::Desktop
    {
        let check = if offline {
            desktop::powershell()
                .map(|path| serde_json::json!({"powershell":path,"com_verified":false}))
        } else {
            DesktopClient.probe().await
        };
        let (healthy, detail) = match check {
            Ok(value) => (true, value),
            Err(error) => (false, serde_json::json!(error.to_string())),
        };
        let value = serde_json::json!({"profile":name,"backend":"desktop","healthy":healthy,"offline":offline,"checks":[{"name":"classic_outlook","ok":healthy,"detail":detail}]});
        return out.value(&value, || {
            format!(
                "Desktop {}: {}{}",
                if healthy {
                    "check passed"
                } else {
                    "check failed"
                },
                detail,
                if offline {
                    " (COM was not checked)"
                } else {
                    ""
                }
            )
        });
    }
    let configured = config::configured_profile(profile_arg);
    let name = configured
        .as_ref()
        .map(|(name, _)| name.as_str())
        .unwrap_or(profile_arg.unwrap_or("default"));
    let signed_in = auth::has_token(name);
    let mut checks = vec![
        serde_json::json!({"name":"configuration","ok":configured.is_some(),"detail":if configured.is_some(){"profile is configured"}else{"run outlook init"}}),
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
        "supported":["read-only keyboard inbox with search, folders, pagination, and message preview","delegated device-code OAuth","personal and work/school accounts","mail listing, reading, search, and field projection","sending, replying, moving, deleting, and read-state updates","draft lifecycle","attachment upload and download up to 150 MiB","calendar agenda and event creation","immutable Outlook IDs","read-only profiles","CLI Spec v0.3"],
        "planned":["browser PKCE login","HTML composition and inline attachments","meeting responses","contacts and categories","delta synchronization and local cache"],
        "api":"Microsoft Graph v1.0",
        "backends":schema::backend_capabilities()
    });
    out.value(&value, || "Graph: full mail lifecycle, attachments, calendar essentials, and device-code OAuth.\nDesktop (Windows/WSL, classic Outlook): folders, mail listing/reading, literal subject/sender search, and draft listing.\nInteractive: outlook tui (read-only inbox, search, folders, and previews).\nPlanned: rich composition, meeting responses, contacts, and delta sync.".into())
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

fn string<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}
fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn confirm_destructive(yes: bool, prompt: &str) -> Result<(), AppError> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        return Err(AppError::ConfirmationRequired(format!(
            "{prompt} Re-run with --yes to confirm."
        )));
    }
    eprint!("{prompt} [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        Err(AppError::ConfirmationRequired(
            "operation cancelled; no changes were made".into(),
        ))
    }
}

fn require_graph(profile: &Profile) -> Result<(), AppError> {
    if profile.backend == BackendKind::Desktop {
        Err(AppError::Unsupported(
            "this operation is not supported by the desktop backend".into(),
        ))
    } else {
        Ok(())
    }
}

// Reject unsupported desktop operations before prompts, stdin reads or file access.
fn require_desktop_command(command: &Command, profile: &Profile) -> Result<(), AppError> {
    let write = match command {
        Command::Calendar { command } => matches!(command, CalendarCommand::Create { .. }),
        Command::Mail { command } => match command {
            MailCommand::Folders { .. }
            | MailCommand::List { .. }
            | MailCommand::Read { .. }
            | MailCommand::Search { .. } => false,
            MailCommand::Draft { command } => !matches!(command, DraftCommand::List(_)),
            MailCommand::Attachment { command } => matches!(
                command,
                AttachmentCommand::Add { .. } | AttachmentCommand::Delete { .. }
            ),
            _ => true,
        },
        _ => false,
    };
    if write {
        profile.require_writable()?;
    }
    let supported = match command {
        Command::Whoami | Command::Calendar { .. } => false,
        Command::Mail { command } => !matches!(command, MailCommand::Attachment { .. }),
        _ => true,
    };
    if supported {
        Ok(())
    } else {
        require_graph(profile)
    }
}
