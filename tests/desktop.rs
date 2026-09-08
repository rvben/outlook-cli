use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn cli(temp: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("outlook").unwrap();
    cmd.env("XDG_CONFIG_HOME", temp.path())
        .env_remove("OUTLOOK_PROFILE")
        .env_remove("OUTLOOK_READ_ONLY")
        .env_remove("OUTLOOK_CLIENT_ID")
        .env_remove("OUTLOOK_TENANT")
        // A desktop command must not accidentally use this Graph credential.
        .env("OUTLOOK_ACCESS_TOKEN", "must-not-use-graph");
    cmd
}

#[test]
fn desktop_configuration_and_profile_lifecycle_need_no_oauth_or_com() {
    let temp = tempfile::tempdir().unwrap();
    cli(&temp)
        .args([
            "--profile",
            "local",
            "init",
            "--backend",
            "desktop",
            "--no-login",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"backend\": \"desktop\""));
    let output = cli(&temp).args(["config", "show"]).output().unwrap();
    let config: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(config["backend"], "desktop");
    assert_eq!(config["client_id"], "");
    cli(&temp)
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"backend\": \"desktop\""));
    let output = cli(&temp)
        .args(["auth", "status", "--offline"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["verified"], false);
    assert_eq!(status["signed_in"], Value::Null);
    for action in ["login", "logout"] {
        cli(&temp)
            .args(["auth", action])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("\"kind\":\"unsupported\""));
    }
    cli(&temp)
        .args(["profile", "remove", "local", "--yes"])
        .assert()
        .success();
}

#[test]
fn unsupported_desktop_commands_fail_before_confirmation_or_reading_stdin() {
    let temp = tempfile::tempdir().unwrap();
    cli(&temp)
        .args(["init", "--backend", "desktop", "--no-login"])
        .assert()
        .success();
    for args in [
        vec![
            "mail",
            "attachment",
            "download",
            "id",
            "attachment",
            "/missing/file",
        ],
        vec!["calendar", "agenda", "--start", "a", "--end", "b"],
        vec!["whoami"],
    ] {
        cli(&temp)
            .args(args)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("\"kind\":\"unsupported\""));
    }
}

#[test]
fn desktop_read_only_policy_is_enforced_before_backend_access() {
    let temp = tempfile::tempdir().unwrap();
    cli(&temp)
        .args(["init", "--backend", "desktop", "--read-only", "--no-login"])
        .assert()
        .success();
    for args in [
        vec!["mail", "mark-read", "id"],
        vec!["mail", "mark-unread", "id"],
        vec!["mail", "move", "id", "--destination", "trash"],
        vec!["mail", "delete", "id"],
        vec!["mail", "reply", "id", "--body", "-"],
        vec![
            "mail",
            "send",
            "--to",
            "person@example.com",
            "--subject",
            "Hello",
            "--body",
            "-",
        ],
        vec!["mail", "draft", "create", "--body", "-"],
        vec!["mail", "draft", "update", "id", "--body", "-"],
        vec!["mail", "draft", "send", "id"],
        vec!["mail", "draft", "delete", "id"],
    ] {
        cli(&temp)
            .args(args)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("\"kind\":\"read_only\""));
    }
}

#[test]
fn desktop_rejects_foreign_ids_cursors_and_empty_queries_before_com_access() {
    let temp = tempfile::tempdir().unwrap();
    cli(&temp)
        .args(["init", "--backend", "desktop", "--no-login"])
        .assert()
        .success();
    for args in [
        vec!["mail", "read", "graph-id"],
        vec!["mail", "mark-read", "graph-id"],
        vec!["mail", "mark-unread", "graph-id"],
        vec!["mail", "move", "graph-id", "--destination", "inbox"],
        vec!["mail", "reply", "graph-id", "--body", "Hello"],
        vec!["mail", "delete", "graph-id", "--yes"],
        vec!["mail", "draft", "update", "graph-id", "--clear-to"],
        vec!["mail", "draft", "send", "graph-id"],
        vec!["mail", "draft", "delete", "graph-id", "--yes"],
        vec!["inbox", "--cursor", "https://graph.microsoft.com/next"],
        vec!["mail", "search", ""],
        vec!["mail", "list", "--folder", "unknown"],
    ] {
        cli(&temp)
            .args(args)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("\"kind\":\"invalid_input\""));
    }
}

#[test]
fn old_profiles_keep_the_graph_backend() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("outlook")).unwrap();
    std::fs::write(
        temp.path().join("outlook/config.toml"),
        "active_profile = 'old'\n[profiles.old]\nclient_id = 'existing-app'\ntenant = 'common'\n",
    )
    .unwrap();
    let output = cli(&temp).args(["config", "show"]).output().unwrap();
    assert!(output.status.success());
    let config: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(config["backend"], "graph");
    assert_eq!(config["client_id"], "existing-app");
}

#[cfg(not(any(windows, target_os = "linux")))]
#[test]
fn unsupported_platform_has_an_actionable_error_and_no_graph_fallback() {
    let temp = tempfile::tempdir().unwrap();
    cli(&temp)
        .args(["init", "--backend", "desktop", "--no-login"])
        .assert()
        .success();
    cli(&temp)
        .arg("inbox")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Windows or WSL"));
    cli(&temp)
        .args(["doctor", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"healthy\": false"));
}

#[test]
fn minimal_desktop_profile_ignores_graph_environment_configuration() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("outlook")).unwrap();
    std::fs::write(
        temp.path().join("outlook/config.toml"),
        "[profiles.default]\nbackend = 'desktop'\n",
    )
    .unwrap();
    let output = cli(&temp)
        .env("OUTLOOK_CLIENT_ID", "graph-app")
        .env("OUTLOOK_TENANT", "graph-tenant")
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let profile: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(profile["backend"], "desktop");
    assert_eq!(profile["client_id"], "");
    assert_eq!(profile["tenant"], "");
}

#[test]
fn desktop_deletes_still_require_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    cli(&temp)
        .args(["init", "--backend", "desktop", "--no-login"])
        .assert()
        .success();
    for args in [
        vec!["mail", "delete", "id"],
        vec!["mail", "draft", "delete", "id"],
    ] {
        cli(&temp)
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "\"kind\":\"confirmation_required\"",
            ));
    }
}
