use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

const CLISPEC_V0_3: &str = include_str!("fixtures/clispec-v0.3.json");

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
    assert_eq!(
        schema["extensions"]["default_client_id"],
        outlook_cli::config::DEFAULT_CLIENT_ID
    );
    let commands = schema["commands"].as_array().unwrap();
    for name in [
        "inbox",
        "mail search",
        "mail mark-read",
        "mail mark-unread",
        "mail delete",
        "mail draft list",
        "mail draft create",
        "mail draft update",
        "mail draft send",
        "mail draft delete",
        "mail attachment list",
        "mail attachment add",
        "mail attachment download",
        "mail attachment delete",
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
fn schema_validates_against_clispec_v0_3() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("outlook")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path())
        .arg("schema")
        .output()
        .unwrap();
    assert!(output.status.success());

    let instance: Value = serde_json::from_slice(&output.stdout).unwrap();
    let specification: Value = serde_json::from_str(CLISPEC_V0_3).unwrap();
    let validator = jsonschema::validator_for(&specification).unwrap();
    let errors = validator
        .iter_errors(&instance)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "schema must validate against CLI Spec v0.3: {}",
        errors.join("; ")
    );
}

#[test]
fn schema_exposes_a_credential_free_representative_example() {
    let schema_output = Command::cargo_bin("outlook")
        .unwrap()
        .arg("schema")
        .output()
        .unwrap();
    assert!(schema_output.status.success());
    let schema: Value = serde_json::from_slice(&schema_output.stdout).unwrap();
    let example_command = schema["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["effects"] == "read_only" && command.get("example").is_some())
        .expect("schema must provide a safe representative example");
    let mut args = example_command["name"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    args.extend(
        example_command["example"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned()),
    );

    let temp = tempfile::tempdir().unwrap();
    let json_output = Command::cargo_bin("outlook")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path())
        .args(&args)
        .output()
        .unwrap();
    assert!(json_output.status.success());
    assert!(json_output.stderr.is_empty());
    assert!(serde_json::from_slice::<Value>(&json_output.stdout).is_ok());

    let text_output = Command::cargo_bin("outlook")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path())
        .args(&args)
        .args(["--output", "text"])
        .output()
        .unwrap();
    assert!(text_output.status.success());
    assert!(text_output.stderr.is_empty());
    assert!(serde_json::from_slice::<Value>(&text_output.stdout).is_err());
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
fn init_rejects_an_explicitly_empty_client_id() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["init", "--client-id=", "--no-login"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("client ID cannot be empty"));
    assert!(!temp.path().join("outlook/config.toml").exists());
}

#[test]
fn init_saves_a_read_only_profile_without_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["init", "--read-only", "--no-login"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["read_only"], true);
    assert_eq!(value["signed_in"], false);
    assert_eq!(value["client_id"], outlook_cli::config::DEFAULT_CLIENT_ID);
    assert!(temp.path().join("outlook/config.toml").is_file());
}

#[test]
fn profile_lifecycle_and_offline_auth_status_are_local() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["work", "personal"] {
        Command::cargo_bin("outlook")
            .unwrap()
            .env("XDG_CONFIG_HOME", temp.path())
            .args(["--profile", name, "init", "--no-login"])
            .assert()
            .success();
    }

    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["profile", "use", "work"])
        .assert()
        .success();

    let list = Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["--output", "json", "profile", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let value: Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(value["total"], 2);
    assert!(
        value["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| { profile["name"] == "work" && profile["active"] == true })
    );

    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["auth", "status", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"verified\": false"));

    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["profile", "remove", "personal", "--yes"])
        .assert()
        .success();
}

#[test]
fn auth_login_bootstraps_the_default_profile() {
    let temp = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable_proxy = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    Command::cargo_bin("outlook")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path())
        .env("HTTPS_PROXY", &unavailable_proxy)
        .env("ALL_PROXY", &unavailable_proxy)
        .args(["auth", "login"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("run `outlook init`").not());

    let output = Command::cargo_bin("outlook")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let profile: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(profile["profile"], "default");
    assert_eq!(profile["tenant"], "common");
    assert_eq!(profile["client_id"], outlook_cli::config::DEFAULT_CLIENT_ID);
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

#[test]
fn deleting_mail_requires_explicit_confirmation_before_authentication() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["mail", "delete", "message-1"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("\"kind\":\"confirmation_required\"")
                .and(predicate::str::contains("--yes")),
        );
}

#[test]
fn attachment_download_never_overwrites_without_force() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("report.pdf");
    std::fs::write(&destination, b"keep me").unwrap();

    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args([
            "mail",
            "attachment",
            "download",
            "message-1",
            "attachment-1",
        ])
        .arg(&destination)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pass --force to replace it"));

    assert_eq!(std::fs::read(destination).unwrap(), b"keep me");
}

#[test]
fn draft_update_requires_at_least_one_change_before_authentication() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["mail", "draft", "update", "draft-1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "draft update requires at least one field",
        ));
}

#[test]
fn read_only_profile_blocks_marking_mail_before_authentication() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["init", "--read-only", "--no-login"])
        .assert()
        .success();

    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["mail", "mark-read", "message-1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"kind\":\"read_only\""));
}

#[test]
fn help_explains_output_and_color_controls() {
    Command::cargo_bin("outlook")
        .unwrap()
        .args(["--no-color", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("JSON when piped")
                .and(predicate::str::contains("--no-color"))
                .and(predicate::str::contains("\x1b").not()),
        );
}

#[test]
fn short_text_flag_also_controls_parse_errors() {
    Command::cargo_bin("outlook")
        .unwrap()
        .args(["-o", "text", "inbox", "--limit", "0"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("error:").and(predicate::str::contains("\"error\":").not()),
        );
}
