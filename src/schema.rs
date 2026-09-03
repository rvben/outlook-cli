use serde_json::{Value, json};

use crate::{auth, error};

fn field(name: &str, kind: &str) -> Value {
    json!({"name":name,"type":kind})
}
fn arg(name: &str, kind: &str, description: &str) -> Value {
    json!({"name":name,"type":kind,"description":description})
}
fn required_arg(name: &str, kind: &str, description: &str) -> Value {
    json!({"name":name,"type":kind,"description":description,"required":true})
}

fn single(name: &str, description: &str, effects: &str, fields: Vec<Value>) -> Value {
    json!({"name":name,"description":description,"effects":effects,"cardinality":"single","output_fields":fields})
}

fn paged(name: &str, description: &str, fields: Vec<Value>) -> Value {
    json!({
        "name":name,
        "description":description,
        "effects":"read_only",
        "cardinality":"unbounded",
        "pagination":{"style":"cursor","cursor_field":"next_cursor","cursor_arg":"--cursor","limit_arg":"--limit"},
        "fields_arg":"--fields",
        "args":[
            arg("--limit","integer","Maximum records in this page"),
            arg("--cursor","string","Opaque continuation URL from the previous page"),
            arg("--fields","string","Comma-separated output fields")
        ],
        "output_fields":fields
    })
}

pub fn generate(command_filter: Option<&str>) -> Value {
    let message_fields = vec![
        field("id", "string"),
        field("subject", "string"),
        field("from", "object"),
        field("receivedDateTime", "string"),
        field("isRead", "boolean"),
        field("hasAttachments", "boolean"),
        field("importance", "string"),
    ];
    let mut commands = vec![
        {
            let mut value = single(
                "init",
                "Configure an Entra public client and optionally sign in",
                "idempotent",
                vec![
                    field("profile", "string"),
                    field("client_id", "string"),
                    field("config_path", "string"),
                    field("signed_in", "boolean"),
                    field("tenant", "string"),
                    field("read_only", "boolean"),
                ],
            );
            value["args"] = json!([
                arg(
                    "--client-id",
                    "string",
                    "Override the bundled Entra public-client application ID"
                ),
                arg(
                    "--tenant",
                    "string",
                    "Tenant ID, domain, common, organizations, or consumers"
                ),
                arg(
                    "--no-login",
                    "boolean",
                    "Save configuration without signing in"
                ),
                arg(
                    "--read-only",
                    "boolean",
                    "Request read scopes and block remote writes"
                )
            ]);
            value
        },
        single(
            "auth login",
            "Sign in with delegated Microsoft device-code OAuth",
            "idempotent",
            vec![
                field("profile", "string"),
                field("expires_at", "integer"),
                field("scope", "string"),
            ],
        ),
        single(
            "auth logout",
            "Remove locally stored delegated credentials",
            "idempotent",
            vec![field("profile", "string"), field("signed_in", "boolean")],
        ),
        single(
            "auth status",
            "Inspect local configuration and credential presence",
            "read_only",
            vec![
                field("profile", "string"),
                field("configured", "boolean"),
                field("signed_in", "boolean"),
                field("read_only", "boolean"),
            ],
        ),
        single(
            "config show",
            "Print the resolved profile without credentials",
            "read_only",
            vec![
                field("profile", "string"),
                field("client_id", "string"),
                field("tenant", "string"),
                field("read_only", "boolean"),
                field("config_path", "string"),
            ],
        ),
        single(
            "config path",
            "Print the absolute configuration file path",
            "read_only",
            vec![],
        ),
        single(
            "whoami",
            "Show the signed-in Microsoft identity",
            "read_only",
            vec![
                field("id", "string"),
                field("displayName", "string"),
                field("userPrincipalName", "string"),
                field("mail", "string"),
            ],
        ),
        paged(
            "inbox",
            "List recent inbox messages",
            message_fields.clone(),
        ),
        {
            let mut value = paged(
                "mail list",
                "List messages in a mail folder",
                message_fields,
            );
            value["args"].as_array_mut().unwrap().insert(
                0,
                arg("--folder", "string", "Well-known folder name or folder ID"),
            );
            value
        },
        {
            let mut value = single(
                "mail read",
                "Read one message",
                "read_only",
                vec![
                    field("id", "string"),
                    field("subject", "string"),
                    field("body", "object"),
                    field("from", "object"),
                ],
            );
            value["args"] = json!([required_arg("id", "string", "Immutable message ID")]);
            value
        },
        {
            let mut value = single(
                "mail send",
                "Send one plain-text message",
                "non_idempotent",
                vec![
                    field("sent", "boolean"),
                    field("to", "array"),
                    field("cc", "array"),
                    field("subject", "string"),
                ],
            );
            value["args"] = json!([
                required_arg("--to", "array", "Recipient address; repeatable"),
                arg("--cc", "array", "CC address; repeatable"),
                required_arg("--subject", "string", "Message subject"),
                required_arg("--body", "string", "Message body, or - for stdin")
            ]);
            value
        },
        {
            let mut value = single(
                "mail reply",
                "Reply to one message",
                "non_idempotent",
                vec![
                    field("sent", "boolean"),
                    field("message_id", "string"),
                    field("reply_all", "boolean"),
                ],
            );
            value["args"] = json!([
                required_arg("id", "string", "Immutable message ID"),
                required_arg("--body", "string", "Reply body, or - for stdin"),
                arg("--all", "boolean", "Reply to all recipients")
            ]);
            value
        },
        {
            let mut value = single(
                "mail move",
                "Move one message",
                "non_idempotent",
                vec![field("id", "string"), field("subject", "string")],
            );
            value["args"] = json!([
                required_arg("id", "string", "Immutable message ID"),
                required_arg(
                    "--destination",
                    "string",
                    "Well-known folder name or folder ID"
                )
            ]);
            value
        },
        {
            let mut value = paged(
                "calendar agenda",
                "List events and occurrences in a time range",
                vec![
                    field("id", "string"),
                    field("subject", "string"),
                    field("start", "object"),
                    field("end", "object"),
                    field("location", "object"),
                ],
            );
            value["args"].as_array_mut().unwrap().splice(
                0..0,
                [
                    required_arg("--start", "string", "Inclusive ISO 8601 start"),
                    required_arg("--end", "string", "Exclusive ISO 8601 end"),
                    arg("--timezone", "string", "Timezone for returned event times"),
                ],
            );
            value
        },
        {
            let mut value = single(
                "calendar create",
                "Create an event in the default calendar",
                "non_idempotent",
                vec![
                    field("id", "string"),
                    field("subject", "string"),
                    field("start", "object"),
                    field("end", "object"),
                ],
            );
            value["args"] = json!([
                required_arg("--subject", "string", "Event subject"),
                required_arg("--start", "string", "Local or ISO 8601 start"),
                required_arg("--end", "string", "Local or ISO 8601 end"),
                arg("--timezone", "string", "Timezone for start and end"),
                arg("--attendee", "array", "Required attendee; repeatable"),
                arg("--body", "string", "Plain-text event description")
            ]);
            value
        },
        single(
            "doctor",
            "Check configuration, credential storage, and Graph access",
            "read_only",
            vec![field("checks", "array"), field("healthy", "boolean")],
        ),
        single(
            "capabilities",
            "Describe supported and planned capabilities",
            "read_only",
            vec![
                field("supported", "array"),
                field("planned", "array"),
                field("api", "string"),
            ],
        ),
        json!({"name":"schema","description":"Emit the offline CLI Spec v0.3 contract","effects":"read_only","cardinality":"single","stdout_schema":{},"args":[arg("--command","string","Return only one complete command path")]}),
        json!({"name":"completions","description":"Generate a shell completion script","effects":"read_only","output_kind":"opaque","media_type":"text/plain","args":[required_arg("shell","string","Shell name")]}),
    ];
    if let Some(filter) = command_filter {
        commands.retain(|command| command["name"] == filter);
    }
    json!({
        "$schema":"https://clispec.dev/schema/v0.3.json",
        "clispec":"0.3",
        "name":"outlook",
        "version":env!("CARGO_PKG_VERSION"),
        "description":"Microsoft Outlook from your terminal, for humans and agents",
        "output":{"tty":"text","piped":"json"},
        "global_args":[
            {"name":"--output","short":"-o","type":"string","enum":["auto","text","json"],"default":"auto","description":"Output format"},
            {"name":"--profile","type":"string","description":"Configuration profile"},
            {"name":"--quiet","type":"boolean","description":"Suppress informational stderr output"}
        ],
        "commands":commands,
        "errors":error::ALL.iter().map(|contract| json!({"kind":contract.kind,"exit_code":contract.exit_code,"retryable":contract.retryable,"description":contract.description})).collect::<Vec<_>>(),
        "extensions":{
            "authentication":"delegated_oauth_device_code",
            "default_client_id":crate::config::DEFAULT_CLIENT_ID,
            "api":"Microsoft Graph v1.0",
            "id_type":"ImmutableId",
            "read_only_env":"OUTLOOK_READ_ONLY",
            "read_scopes":auth::READ_SCOPES.split_whitespace().collect::<Vec<_>>(),
            "write_scopes":auth::WRITE_SCOPES.split_whitespace().collect::<Vec<_>>()
        }
    })
}
