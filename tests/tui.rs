use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn tui_refuses_piped_input_before_authentication() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env("XDG_CONFIG_HOME", temp.path())
        .arg("tui")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("tty_required"));
}

#[test]
fn sample_snapshot_is_offline_and_uses_the_real_layout() {
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("outlook")
        .unwrap()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("XDG_CONFIG_HOME", temp.path())
        .args(["tui", "--demo", "--snapshot", "--no-color"])
        .assert()
        .success()
        .stderr("")
        .stdout(
            predicate::str::contains("DEMO · sample mail")
                .and(predicate::str::contains("revised layouts"))
                .and(predicate::str::contains("\x1b").not()),
        );
    assert!(!temp.path().join("outlook/config.toml").exists());
}

#[test]
fn snapshot_requires_demo_and_bounded_dimensions() {
    for args in [
        vec!["tui", "--snapshot"],
        vec!["tui", "--demo", "--snapshot", "--width", "0"],
        vec!["tui", "--demo", "--snapshot", "--height", "1000"],
    ] {
        Command::cargo_bin("outlook")
            .unwrap()
            .args(args)
            .assert()
            .code(2);
    }
}

#[test]
fn interactive_mode_rejects_explicit_json() {
    Command::cargo_bin("outlook")
        .unwrap()
        .args(["tui", "--demo", "--output", "json"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("invalid_input"));
}

#[test]
fn schema_exposes_read_only_tui_and_offline_snapshot() {
    let output = Command::cargo_bin("outlook")
        .unwrap()
        .args(["schema", "--command", "tui"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["commands"][0]["effects"], "read_only");
    assert_eq!(
        schema["commands"][0]["extensions"]["preview_marks_read"],
        false
    );
}
