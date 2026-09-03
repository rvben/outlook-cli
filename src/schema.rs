use serde_json::{Value, json};

use crate::{auth, error};

fn field(name: &str, kind: &str) -> Value {
    json!({"name":name,"type":kind})
}
fn array_field(name: &str, item_kind: &str) -> Value {
    json!({"name":name,"type":"array","items":{"type":item_kind}})
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
        {
            let mut value = single(
                "config path",
                "Print the absolute configuration file path",
                "read_only",
                vec![],
            );
            value["example"] = json!({"args":[]});
            value
        },
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
                message_fields.clone(),
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
            let mut value = paged(
                "mail search",
                "Search messages across the mailbox or within one folder",
                vec![
                    field("id", "string"),
                    field("subject", "string"),
                    field("from", "object"),
                    field("receivedDateTime", "string"),
                    field("isRead", "boolean"),
                    field("hasAttachments", "boolean"),
                    field("importance", "string"),
                ],
            );
            value["args"].as_array_mut().unwrap().splice(
                0..0,
                [
                    required_arg("query", "string", "Search text or KQL expression"),
                    arg("--folder", "string", "Well-known folder name or folder ID"),
                ],
            );
            value
        },
        {
            let mut value = single(
                "mail mark-read",
                "Mark one message as read",
                "idempotent",
                vec![field("id", "string"), field("isRead", "boolean")],
            );
            value["args"] = json!([required_arg("id", "string", "Immutable message ID")]);
            value
        },
        {
            let mut value = single(
                "mail mark-unread",
                "Mark one message as unread",
                "idempotent",
                vec![field("id", "string"), field("isRead", "boolean")],
            );
            value["args"] = json!([required_arg("id", "string", "Immutable message ID")]);
            value
        },
        {
            let mut value = single(
                "mail delete",
                "Delete one message after confirmation",
                "idempotent",
                vec![field("deleted", "boolean"), field("message_id", "string")],
            );
            value["args"] = json!([required_arg("id", "string", "Immutable message ID")]);
            value["confirmation_bypass_arg"] = json!("--yes");
            value
        },
        paged("mail draft list", "List saved drafts", message_fields),
        {
            let mut value = single(
                "mail draft create",
                "Create a saved plain-text draft",
                "non_idempotent",
                vec![
                    field("id", "string"),
                    field("subject", "string"),
                    field("isDraft", "boolean"),
                ],
            );
            value["args"] = json!([
                arg("--to", "array", "Recipient address; repeatable"),
                arg("--cc", "array", "CC address; repeatable"),
                arg("--bcc", "array", "BCC address; repeatable"),
                arg("--subject", "string", "Message subject"),
                arg("--body", "string", "Message body, or - for stdin")
            ]);
            value
        },
        {
            let mut value = single(
                "mail draft update",
                "Update selected fields on a saved draft",
                "idempotent",
                vec![
                    field("id", "string"),
                    field("subject", "string"),
                    field("isDraft", "boolean"),
                ],
            );
            value["args"] = json!([
                required_arg("id", "string", "Immutable draft ID"),
                arg("--to", "array", "Replace To recipients; repeatable"),
                arg("--clear-to", "boolean", "Remove all To recipients"),
                arg("--cc", "array", "Replace CC recipients; repeatable"),
                arg("--clear-cc", "boolean", "Remove all CC recipients"),
                arg("--bcc", "array", "Replace BCC recipients; repeatable"),
                arg("--clear-bcc", "boolean", "Remove all BCC recipients"),
                arg("--subject", "string", "Replace the subject"),
                arg("--body", "string", "Replace the body, or - for stdin")
            ]);
            value
        },
        {
            let mut value = single(
                "mail draft send",
                "Send an existing draft",
                "non_idempotent",
                vec![field("sent", "boolean"), field("draft_id", "string")],
            );
            value["args"] = json!([required_arg("id", "string", "Immutable draft ID")]);
            value
        },
        {
            let mut value = single(
                "mail draft delete",
                "Delete an existing draft after confirmation",
                "idempotent",
                vec![field("deleted", "boolean"), field("message_id", "string")],
            );
            value["args"] = json!([required_arg("id", "string", "Immutable draft ID")]);
            value["confirmation_bypass_arg"] = json!("--yes");
            value
        },
        {
            let mut value = paged(
                "mail attachment list",
                "List attachment metadata without downloading content",
                vec![
                    field("id", "string"),
                    field("name", "string"),
                    field("contentType", "string"),
                    field("size", "integer"),
                    field("isInline", "boolean"),
                ],
            );
            value["args"].as_array_mut().unwrap().insert(
                0,
                required_arg("message_id", "string", "Immutable message ID"),
            );
            value
        },
        {
            let mut value = single(
                "mail attachment add",
                "Attach a local file to a draft, up to 150 MiB",
                "non_idempotent",
                vec![
                    field("id", "string"),
                    field("name", "string"),
                    field("size", "integer"),
                ],
            );
            value["args"] = json!([
                required_arg("message_id", "string", "Immutable draft ID"),
                required_arg("path", "string", "Local file path"),
                arg(
                    "--content-type",
                    "string",
                    "Override the attachment MIME type"
                )
            ]);
            value
        },
        {
            let mut value = single(
                "mail attachment download",
                "Download one attachment without overwriting by default",
                "idempotent",
                vec![
                    field("message_id", "string"),
                    field("attachment_id", "string"),
                    field("path", "string"),
                    field("size", "integer"),
                ],
            );
            value["args"] = json!([
                required_arg("message_id", "string", "Immutable message ID"),
                required_arg("attachment_id", "string", "Attachment ID"),
                required_arg("path", "string", "Local destination path"),
                arg("--force", "boolean", "Replace an existing destination file")
            ]);
            value
        },
        {
            let mut value = single(
                "mail attachment delete",
                "Delete one attachment after confirmation",
                "idempotent",
                vec![
                    field("deleted", "boolean"),
                    field("message_id", "string"),
                    field("attachment_id", "string"),
                ],
            );
            value["args"] = json!([
                required_arg("message_id", "string", "Immutable message ID"),
                required_arg("attachment_id", "string", "Attachment ID")
            ]);
            value["confirmation_bypass_arg"] = json!("--yes");
            value
        },
        {
            let mut value = single(
                "mail send",
                "Send one plain-text message",
                "non_idempotent",
                vec![
                    field("sent", "boolean"),
                    array_field("to", "string"),
                    array_field("cc", "string"),
                    array_field("bcc", "string"),
                    field("subject", "string"),
                ],
            );
            value["args"] = json!([
                required_arg("--to", "array", "Recipient address; repeatable"),
                arg("--cc", "array", "CC address; repeatable"),
                arg("--bcc", "array", "BCC address; repeatable"),
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
            vec![array_field("checks", "object"), field("healthy", "boolean")],
        ),
        single(
            "capabilities",
            "Describe supported and planned capabilities",
            "read_only",
            vec![
                array_field("supported", "string"),
                array_field("planned", "string"),
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
            {"name":"--quiet","type":"boolean","description":"Suppress informational stderr output"},
            {"name":"--yes","short":"-y","type":"boolean","description":"Skip confirmation prompts for destructive operations"}
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
