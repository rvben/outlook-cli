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
    let temp = tempfile::tempdir().unwrap();
    let script_path = temp.path().join("bridge.ps1");
    // The mock plus production script exceeds the Windows encoded-command limit.
    // A BOM keeps the fixture's Unicode intact under Windows PowerShell 5.1.
    std::fs::write(&script_path, format!("\u{feff}{script}")).unwrap();
    let state_path = temp.path().join("state.json");
    let executable =
        std::env::var_os("OUTLOOK_TEST_POWERSHELL").unwrap_or_else(|| "powershell.exe".into());
    let mut command = Command::new(executable);
    command.args(["-NoLogo", "-NoProfile", "-NonInteractive"]);
    if cfg!(windows) {
        command.args(["-STA", "-ExecutionPolicy", "Bypass"]);
    }
    let mut child = command
        .arg("-File")
        .arg(&script_path)
        .env("OUTLOOK_MOCK_STATE", &state_path)
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
    let mut value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}').trim())
        .unwrap_or_else(|error| {
            panic!(
                "{error}: stdout={text}, stderr={}",
                String::from_utf8_lossy(&result.stderr)
            )
        });
    if state_path.exists() {
        value["_mock"] =
            serde_json::from_str(&std::fs::read_to_string(state_path).unwrap()).unwrap();
    }
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

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires PowerShell; set OUTLOOK_TEST_POWERSHELL and run --ignored"
)]
fn powershell_bridge_writes_mail_with_mock_outlook() {
    let id = json!({"entry":"AB01","store":"AABB"});
    for read in [true, false] {
        let (ok, value) = bridge(json!({"operation":"mark_read","id":id,"read":read}));
        assert!(ok, "{value}");
        assert_eq!(value["isRead"], read);
        assert_eq!(value["_mock"]["action"], "save");
        assert_eq!(value["_mock"]["unread"], !read);
    }
    for folder in [json!(5), json!({"entry":"F003","store":"CCDD"})] {
        let (ok, value) = bridge(json!({"operation":"move","id":id,"folder":folder}));
        assert!(ok, "{value}");
        let moved: Value = serde_json::from_slice(
            &STANDARD
                .decode(
                    value["id"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("desktop:")
                        .unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(moved["entry"], "AB99");
        assert_eq!(
            moved["store"],
            if folder.is_number() { "AABB" } else { "CCDD" }
        );
        assert_eq!(value["_mock"]["action"], "move");
    }
    let (ok, value) = bridge(json!({"operation":"delete","id":id}));
    assert!(ok, "{value}");
    assert_eq!(value["deleted"], true);
    assert_eq!(value["_mock"]["action"], "delete");
    for all in [false, true] {
        let (ok, value) =
            bridge(json!({"operation":"reply","id":id,"body":"Thanks 日本語 $(exit)","all":all}));
        assert!(ok, "{value}");
        assert_eq!(value["sent"], true);
        assert_eq!(value["reply_all"], all);
        assert_eq!(value["_mock"]["action"], "send");
        assert_eq!(value["_mock"]["reply"], if all { "all" } else { "reply" });
        assert_eq!(
            value["_mock"]["body"],
            "Thanks 日本語 $(exit)\r\n\r\nFull message body"
        );
    }
    for operation in ["send", "draft_create"] {
        let (ok, value) = bridge(
            json!({"operation":operation,"to":["to@example.com"],"cc":["cc@example.com"],"bcc":["bcc@example.com"],"subject":"O'Brien; 日本語", "body":"`test` $(exit)\nhello"}),
        );
        assert!(ok, "{value}");
        assert_eq!(
            value["_mock"]["action"],
            if operation == "send" { "send" } else { "save" }
        );
        assert_eq!(value["_mock"]["subject"], "O'Brien; 日本語");
        assert_eq!(value["_mock"]["body"], "`test` $(exit)\nhello");
        for (index, address) in ["to@example.com", "cc@example.com", "bcc@example.com"]
            .iter()
            .enumerate()
        {
            assert_eq!(value["_mock"]["recipients"][index]["Address"], *address);
            assert_eq!(value["_mock"]["recipients"][index]["Type"], index + 1);
        }
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires PowerShell; set OUTLOOK_TEST_POWERSHELL and run --ignored"
)]
fn powershell_bridge_edits_drafts_and_rejects_invalid_writes() {
    let id = json!({"entry":"DA01","store":"AABB"});
    let (ok, value) = bridge(
        json!({"operation":"draft_update","id":id,"to":[],"cc":null,"bcc":["new@example.com"],"subject":"","body":""}),
    );
    assert!(ok, "{value}");
    assert_eq!(value["subject"], "");
    assert_eq!(value["body"]["content"], "");
    assert_eq!(
        value["_mock"]["recipients"],
        json!([
            {"Address":"copy@example.com","Type":2},
            {"Address":"new@example.com","Type":3}
        ])
    );
    let (ok, value) = bridge(json!({"operation":"draft_update","id":id,"subject":"Updated"}));
    assert!(ok, "{value}");
    assert_eq!(value["body"]["content"], "Full message body");
    assert_eq!(value["_mock"]["recipients"].as_array().unwrap().len(), 3);
    for operation in ["draft_send", "draft_delete"] {
        let (ok, value) = bridge(json!({"operation":operation,"id":id}));
        assert!(ok, "{value}");
        assert_eq!(
            value["_mock"]["action"],
            if operation == "draft_send" {
                "send"
            } else {
                "delete"
            }
        );
    }
    let (ok, value) =
        bridge(json!({"operation":"draft_create","to":[],"cc":[],"bcc":[],"subject":"","body":""}));
    assert!(ok, "{value}");
    assert_eq!(value["isDraft"], true);
    for entry in ["AB01", "DA02", "CA01"] {
        for operation in ["draft_update", "draft_send", "draft_delete"] {
            let (ok, value) = bridge(
                json!({"operation":operation,"id":{"entry":entry,"store":"AABB"},"subject":"Do not change"}),
            );
            assert!(!ok, "{value}");
            assert_eq!(value["error"]["kind"], "invalid_input");
            assert!(value.get("_mock").is_none(), "unexpected mutation: {value}");
        }
    }
    for request in [
        json!({"operation":"draft_send","id":{"entry":"DA03","store":"AABB"}}),
        json!({"operation":"send","to":["unresolved@example.com"]}),
        json!({"operation":"move","id":{"entry":"AB01","store":"AABB"},"folder":{"entry":"F004","store":"AABB"}}),
    ] {
        let (ok, value) = bridge(request);
        assert!(!ok, "{value}");
        assert_eq!(value["error"]["kind"], "invalid_input");
        assert!(value.get("_mock").is_none(), "unexpected mutation: {value}");
    }
}
