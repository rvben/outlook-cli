use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

#[test]
fn schema_is_offline_and_describes_the_core_surface() {
    let output = Command::cargo_bin("outlook")
        .unwrap()
        .arg("schema")
        .output()
        .unwrap();
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["clispec"], "0.3");
    assert_eq!(schema["name"], "outlook");
    assert_eq!(schema["extensions"]["id_type"], "ImmutableId");
    let commands = schema["commands"].as_array().unwrap();
    for name in [
        "inbox",
        "mail send",
        "mail reply",
        "calendar agenda",
        "calendar create",
    ] {
        assert!(
            commands.iter().any(|command| command["name"] == name),
            "missing {name}"
        );
    }
}

#[test]
fn schema_can_select_one_command() {
    Command::cargo_bin("outlook")
        .unwrap()
        .args(["schema", "--command", "mail send"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"name\": \"mail send\"")
                .and(predicate::str::contains("calendar create").not()),
        );
}

#[test]
fn init_requires_a_real_client_id() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["init", "--no-login"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "public-client application ID is required",
        ));
    assert!(!temp.path().join("outlook/config.toml").exists());
}

#[test]
fn init_saves_a_read_only_profile_without_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args([
            "init",
            "--client-id",
            "00000000-0000-0000-0000-000000000001",
            "--read-only",
            "--no-login",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["read_only"], true);
    assert_eq!(value["signed_in"], false);
    assert!(temp.path().join("outlook/config.toml").is_file());
}

#[test]
fn no_args_never_prompts_when_piped() {
    Command::cargo_bin("outlook")
        .unwrap()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"kind\":\"tty_required\""));
}

#[test]
fn invalid_email_is_rejected_before_authentication() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args([
            "mail",
            "send",
            "--to",
            "not-an-address",
            "--subject",
            "hello",
            "--body",
            "test",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid email address"));
}
