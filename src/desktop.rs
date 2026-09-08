//! Classic Outlook automation through a fixed PowerShell script and JSON stdin.
use crate::{error::AppError, graph::Page};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

const SCRIPT: &str = include_str!("desktop.ps1");
const TIMEOUT: Duration = Duration::from_secs(45);
pub struct DesktopClient;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    operation: String,
    folder: Option<String>,
    query: Option<String>,
    offset: u32,
}

pub fn powershell() -> Result<PathBuf, AppError> {
    let supported = cfg!(windows)
        || (cfg!(target_os = "linux")
            && std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("microsoft"));
    if !supported {
        return Err(AppError::Unsupported("desktop requires Windows or WSL with Windows interop, and classic Outlook (new Outlook has no COM support)".into()));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|path| path.join("powershell.exe"))
        .find(|path| path.is_file())
        .ok_or_else(|| AppError::Desktop("powershell.exe was not found on PATH; enable Windows interop in WSL and install Windows PowerShell".into()))
}

fn decode_id(id: &str) -> Result<Value, AppError> {
    let invalid = || {
        AppError::InvalidInput("expected a desktop ID returned by this backend (Graph IDs and inbox indexes cannot be used)".into())
    };
    if id.len() > 32768 {
        return Err(invalid());
    }
    let bytes = STANDARD
        .decode(id.strip_prefix("desktop:").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    for key in ["entry", "store"] {
        let text = value[key].as_str().ok_or_else(invalid)?;
        if text.is_empty() || text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
    }
    Ok(value)
}

fn folder_value(folder: Option<&str>) -> Result<Value, AppError> {
    match folder {
        None => Ok(Value::Null),
        Some(id) if id.starts_with("desktop:") => decode_id(id),
        Some(name) => match name.to_ascii_lowercase().as_str() {
            "inbox" => Ok(json!(6)), "sentitems" | "sent" => Ok(json!(5)),
            "deleteditems" | "trash" => Ok(json!(3)), "outbox" => Ok(json!(4)),
            "drafts" => Ok(json!(16)), "junkemail" | "junk" => Ok(json!(23)),
            _ => Err(AppError::InvalidInput("desktop folder must be inbox, sentitems, deleteditems, outbox, drafts, junkemail, or a desktop folder ID from `mail folders`".into())),
        }
    }
}

fn offset(cursor: Option<&str>, expected: &Cursor) -> Result<u32, AppError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let invalid = || {
        AppError::InvalidInput(
            "invalid desktop cursor or cursor does not match this folder/search".into(),
        )
    };
    if cursor.len() > 65536 {
        return Err(invalid());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor.strip_prefix("desktop-page:").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let actual: Cursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if actual.operation != expected.operation
        || actual.folder != expected.folder
        || actual.query != expected.query
        || actual.offset > i32::MAX as u32 - 1001
    {
        return Err(invalid());
    }
    Ok(actual.offset)
}

impl DesktopClient {
    pub async fn send_mail(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        self.run(json!({"operation":"send", "to":to, "cc":cc, "bcc":bcc, "subject":subject, "body":body})).await
    }
    pub async fn create_draft(
        &self,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body: &str,
    ) -> Result<Value, AppError> {
        self.run(json!({"operation":"draft_create", "to":to, "cc":cc, "bcc":bcc, "subject":subject, "body":body})).await
    }
    pub async fn update_draft(
        &self,
        id: &str,
        to: Option<&[String]>,
        cc: Option<&[String]>,
        bcc: Option<&[String]>,
        subject: Option<&str>,
        body: Option<&str>,
    ) -> Result<Value, AppError> {
        self.run(json!({"operation":"draft_update", "id":decode_id(id)?, "to":to, "cc":cc, "bcc":bcc, "subject":subject, "body":body})).await
    }
    pub async fn send_draft(&self, id: &str) -> Result<Value, AppError> {
        self.run(json!({"operation":"draft_send", "id":decode_id(id)?}))
            .await
    }
    pub async fn reply(&self, id: &str, body: &str, all: bool) -> Result<Value, AppError> {
        self.run(json!({"operation":"reply", "id":decode_id(id)?, "body":body, "all":all}))
            .await
    }
    pub async fn move_message(&self, id: &str, destination: &str) -> Result<Value, AppError> {
        self.run(json!({"operation":"move", "id":decode_id(id)?, "folder":folder_value(Some(destination))?})).await
    }
    pub async fn set_message_read(&self, id: &str, read: bool) -> Result<Value, AppError> {
        self.run(json!({"operation":"mark_read", "id":decode_id(id)?, "read":read}))
            .await
    }
    pub async fn delete_message(&self, id: &str) -> Result<Value, AppError> {
        self.run(json!({"operation":"delete", "id":decode_id(id)?}))
            .await
    }
    pub async fn delete_draft(&self, id: &str) -> Result<Value, AppError> {
        self.run(json!({"operation":"draft_delete", "id":decode_id(id)?}))
            .await
    }

    pub async fn probe(&self) -> Result<Value, AppError> {
        self.run(json!({"operation":"probe"})).await
    }
    pub async fn message(&self, id: &str) -> Result<Value, AppError> {
        self.run(json!({"operation":"read", "id":decode_id(id)?}))
            .await
    }
    pub async fn page(
        &self,
        operation: &str,
        folder: Option<&str>,
        query: Option<&str>,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Page, AppError> {
        if !matches!(operation, "list" | "search" | "folders") || !(1..=100).contains(&limit) {
            return Err(AppError::InvalidInput(
                "invalid desktop page operation or limit".into(),
            ));
        }
        if query.is_some_and(|q| q.trim().is_empty() || q.len() > 8192) {
            return Err(AppError::InvalidInput(
                "desktop search must contain 1–8192 bytes of literal text".into(),
            ));
        }
        let mut state = Cursor {
            operation: operation.into(),
            folder: folder.map(str::to_owned),
            query: query.map(str::to_owned),
            offset: 0,
        };
        state.offset = offset(cursor, &state)?;
        let value = self.run(json!({"operation":operation, "folder":folder_value(folder)?, "query":query, "limit":limit, "offset":state.offset})).await?;
        let items = value["items"]
            .as_array()
            .ok_or_else(|| AppError::Desktop("bridge returned an invalid page".into()))?
            .clone();
        let next_cursor = match value["next_offset"].as_u64() {
            Some(next) if next > u64::from(state.offset) && next <= (i32::MAX - 1001) as u64 => {
                state.offset = next as u32;
                Some(format!(
                    "desktop-page:{}",
                    URL_SAFE_NO_PAD.encode(
                        serde_json::to_vec(&state)
                            .map_err(|e| AppError::Unexpected(e.to_string()))?
                    )
                ))
            }
            None if value["next_offset"].is_null() => None,
            _ => {
                return Err(AppError::Desktop(
                    "bridge returned an invalid continuation offset".into(),
                ));
            }
        };
        Ok(Page {
            items,
            truncated: next_cursor.is_some(),
            next_cursor,
        })
    }
    async fn run(&self, request: Value) -> Result<Value, AppError> {
        let executable = powershell()?;
        run_bridge(Command::new(executable), request, TIMEOUT).await
    }
}

async fn run_bridge(
    mut command: Command,
    request: Value,
    timeout: Duration,
) -> Result<Value, AppError> {
    // Only the bundled script is executable code. All user data travels over stdin.
    let encoded = STANDARD.encode(
        SCRIPT
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let mut child = command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-EncodedCommand",
            &encoded,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| AppError::Desktop(format!("cannot start Windows PowerShell: {e}")))?;
    let bytes = serde_json::to_vec(&request).map_err(|e| AppError::Unexpected(e.to_string()))?;
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Desktop("bridge stdin is unavailable".into()))?;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|e| AppError::Desktop(format!("cannot send bridge request: {e}")))?;
        drop(stdin);
        child
            .wait_with_output()
            .await
            .map_err(|e| AppError::Desktop(format!("cannot wait for bridge: {e}")))
    };
    let output = tokio::time::timeout(timeout, operation).await
        .map_err(|_| AppError::Desktop("classic Outlook did not respond within the bridge timeout; check Windows for Outlook profile or security prompts. A write may already have completed; inspect Outlook before retrying".into()))??;
    parse_output(output.status.success(), &output.stdout, &output.stderr)
}

fn parse_output(success: bool, stdout: &[u8], stderr: &[u8]) -> Result<Value, AppError> {
    let text = String::from_utf8_lossy(stdout);
    let value: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}').trim()).map_err(|_| {
            AppError::Desktop(format!(
                "invalid PowerShell response: {}",
                String::from_utf8_lossy(stderr).trim()
            ))
        })?;
    if let Some(error) = value.get("error") {
        let message = error["message"]
            .as_str()
            .unwrap_or("Outlook COM operation failed")
            .to_owned();
        return Err(if error["kind"] == "not_found" {
            AppError::NotFound(message)
        } else if error["kind"] == "invalid_input" {
            AppError::InvalidInput(message)
        } else {
            AppError::Desktop(message)
        });
    }
    if !success {
        return Err(AppError::Desktop(format!(
            "PowerShell failed: {}",
            String::from_utf8_lossy(stderr).trim()
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_script_fits_windows_command_line() {
        let encoded_len = SCRIPT.encode_utf16().count().saturating_mul(2).div_ceil(3) * 4;
        assert!(encoded_len < 32000, "leave room for executable and flags");
    }

    #[test]
    fn ids_require_both_hex_entry_and_store_ids() {
        let id = format!(
            "desktop:{}",
            STANDARD.encode(br#"{"entry":"ABCD","store":"1234"}"#)
        );
        assert_eq!(decode_id(&id).unwrap()["store"], "1234");
        for bad in [
            "1".to_owned(),
            "graph-id".to_owned(),
            "desktop:bad".to_owned(),
            format!("desktop:{}", STANDARD.encode(br#"{"entry":"ABCD"}"#)),
            format!(
                "desktop:{}",
                STANDARD.encode(br#"{"entry":"';exit;#","store":"12"}"#)
            ),
        ] {
            assert!(decode_id(&bad).is_err(), "accepted {bad}");
        }
        assert_eq!(folder_value(Some("sentitems")).unwrap(), 5);
        assert!(folder_value(Some("6; exit")).is_err());
    }

    #[test]
    fn cursors_cannot_be_reused_for_another_search_or_backend() {
        let mut state = Cursor {
            operation: "search".into(),
            folder: Some("inbox".into()),
            query: Some("hello".into()),
            offset: 10,
        };
        let token = format!(
            "desktop-page:{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&state).unwrap())
        );
        assert_eq!(offset(Some(&token), &state).unwrap(), 10);
        state.query = Some("other".into());
        assert!(offset(Some(&token), &state).is_err());
        assert!(offset(Some("https://graph.microsoft.com/next"), &state).is_err());
        state.query = Some("hello".into());
        state.folder = Some("drafts".into());
        assert!(offset(Some(&token), &state).is_err());
    }

    #[test]
    fn bridge_errors_remain_structured_and_diagnostics_are_preserved() {
        let error = parse_output(
            false,
            br#"{"error":{"kind":"not_found","message":"Gone"}}"#,
            b"",
        )
        .unwrap_err();
        assert_eq!(error.contract().kind, "not_found");
        assert_eq!(error.to_string(), "Gone");
        let error = parse_output(false, b"", b"PowerShell startup failed").unwrap_err();
        assert!(error.to_string().contains("PowerShell startup failed"));
        assert!(parse_output(true, b"noise", b"").is_err());
        assert_eq!(
            parse_output(true, "\u{feff}{\"items\":[]}\r\n".as_bytes(), b"").unwrap()["items"],
            json!([])
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_transports_untrusted_unicode_as_data() {
        let mut command = Command::new("/bin/sh");
        // Assert the invocation shape, then act as a JSON echo bridge.
        command.args([
            "-c",
            "test \"$5\" = '-EncodedCommand' || exit 8; exec cat",
            "bridge-test",
        ]);
        let request = json!({"query":"O'Brien; $(exit 7) `hello` \" \\ café 日本語\n[] .*"});
        assert_eq!(
            run_bridge(command, request.clone(), Duration::from_secs(5))
                .await
                .unwrap(),
            request
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hung_bridge_times_out() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 10", "bridge-test"]);
        let error = run_bridge(command, json!({}), Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timeout"));
    }
}
