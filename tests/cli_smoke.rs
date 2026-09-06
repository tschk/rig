use assert_cmd::Command;
use predicates::prelude::*;

fn rig() -> Command {
    Command::cargo_bin("rig").unwrap()
}

#[test]
fn help_lists_core_commands() {
    rig()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("upgrade"))
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn add_help_shows_lang_flags() {
    rig()
        .arg("add")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--cargo"))
        .stdout(predicate::str::contains("--rust"))
        .stdout(predicate::str::contains("--zig"))
        .stdout(predicate::str::contains("--nim"))
        .stdout(predicate::str::contains("--csharp"));
}

#[test]
fn aliases_parse() {
    for alias in [
        "i",
        "install",
        "rm",
        "ui",
        "uninstall",
        "up",
        "ls",
        "s",
        "find",
        "show",
        "dr",
        "doctor",
    ] {
        let mut cmd = rig();
        let assert = if matches!(alias, "rm" | "ui" | "uninstall" | "show") {
            cmd.args(["help", alias]).assert()
        } else {
            cmd.args([alias, "--help"]).assert()
        };
        assert.success();
    }
}

#[test]
fn init_and_list_in_temp() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"demo\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    assert!(dir.path().join("rig.toml").is_file());

    rig()
        .current_dir(dir.path())
        .args(["ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no dependencies"));
}
