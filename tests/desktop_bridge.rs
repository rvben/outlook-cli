//! Executes the production PowerShell code with only COM creation replaced.
//! Windows CI uses PowerShell 5.1. Elsewhere run explicitly with a portable pwsh.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn bridge(request: Value) -> (bool, Value) {
    let script = format!(
        "{}\n{}",
        include_str!("fixtures/desktop-mock.ps1"),
        include_str!("../src/desktop.ps1").replace(
            "New-Object -ComObject Outlook.Application",
            "New-MockApplication"
        )
    );
    let encoded = STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    assert!(
        encoded.len() < 32000,
        "script must fit the Windows command line"
    );
    let executable =
        std::env::var_os("OUTLOOK_TEST_POWERSHELL").unwrap_or_else(|| "powershell.exe".into());
    let mut command = Command::new(executable);
    command.args(["-NoLogo", "-NoProfile", "-NonInteractive"]);
    if cfg!(windows) {
        command.arg("-STA");
    }
    let mut child = command
        .args(["-EncodedCommand", &encoded])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&result.stdout);
    let value =
        serde_json::from_str(text.trim_start_matches('\u{feff}').trim()).unwrap_or_else(|error| {
            panic!(
                "{error}: stdout={text}, stderr={}",
                String::from_utf8_lossy(&result.stderr)
            )
        });
    (result.status.success(), value)
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires PowerShell; set OUTLOOK_TEST_POWERSHELL and run --ignored"
)]
fn powershell_bridge_lists_reads_searches_and_pages_with_mock_outlook() {
    let (ok, probe) = bridge(json!({"operation":"probe"}));
    assert!(ok, "{probe}");
    assert_eq!(probe["defaultStore"], "Test mailbox");
    let (ok, folders) = bridge(json!({"operation":"folders","folder":null,"offset":0,"limit":10}));
    assert!(ok, "{folders}");
    assert_eq!(folders["items"].as_array().unwrap().len(), 1);
    assert_eq!(folders["items"][0]["displayName"], "Inbox");
    let (ok, page) = bridge(json!({"operation":"list","folder":6,"offset":0,"limit":1}));
    assert!(ok, "{page}");
    assert_eq!(page["items"][0]["subject"], "O'Brien [report]");
    assert_eq!(page["items"][0]["isRead"], false);
    assert!(page["items"][0].get("body").is_none());
    assert_eq!(page["next_offset"], 1);
    let bytes = STANDARD
        .decode(
            page["items"][0]["id"]
                .as_str()
                .unwrap()
                .strip_prefix("desktop:")
                .unwrap(),
        )
        .unwrap();
    let id: Value = serde_json::from_slice(&bytes).unwrap();
    let (ok, mail) = bridge(json!({"operation":"read","id":id}));
    assert!(ok, "{mail}");
    assert_eq!(mail["body"]["content"], "Full message body");
    for query in ["o'BRIEN [report]", "SENDER@EXAMPLE.COM"] {
        let (ok, search) =
            bridge(json!({"operation":"search","folder":6,"query":query,"offset":0,"limit":1}));
        assert!(ok, "{search}");
        assert_eq!(search["items"][0]["id"], page["items"][0]["id"]);
    }
    let (ok, empty) = bridge(
        json!({"operation":"search","folder":6,"query":"'; throw 'injected'; # 日本語 .*","offset":0,"limit":10}),
    );
    assert!(ok, "{empty}");
    assert_eq!(empty["items"], json!([]));
    assert_eq!(empty["next_offset"], 1000);
    let (ok, end) = bridge(
        json!({"operation":"search","folder":6,"query":"not present","offset":1000,"limit":10}),
    );
    assert!(ok, "{end}");
    assert_eq!(end["items"], json!([]));
    assert_eq!(end["next_offset"], Value::Null);
    let (ok, missing) = bridge(json!({"operation":"read","id":{"entry":"FFFF","store":"AABB"}}));
    assert!(!ok);
    assert_eq!(missing["error"]["kind"], "not_found");
}
