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

#[test]
fn search_path_git_ecosystem_is_honest() {
    rig()
        .args(["search", "--odin", "foo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no central registry"))
        .stdout(predicate::str::contains("path:"));
}

#[test]
fn add_path_non_cargo_updates_manifest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"demo\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
    let vendor = dir.path().join("vendor/mylib");
    std::fs::create_dir_all(&vendor).unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--c", &path_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let manifest = std::fs::read_to_string(dir.path().join("rig.toml")).unwrap();
    assert!(
        manifest.contains("mylib"),
        "manifest should list mylib: {manifest}"
    );
    assert!(
        manifest.contains("ecosystem") || manifest.contains("c"),
        "{manifest}"
    );
}

#[test]
fn add_path_c_on_c_host_builds_native() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "all:\n\t@echo ok\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.c"), "int main(void){return 0;}\n").unwrap();

    let vendor = dir.path().join("vendor/mylib");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("mylib.h"),
        "#pragma once\nint mylib_add(int a, int b);\n",
    )
    .unwrap();
    std::fs::write(
        vendor.join("mylib.c"),
        "#include \"mylib.h\"\nint mylib_add(int a, int b) { return a + b; }\n",
    )
    .unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: c"));

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--c", &path_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let header = dir.path().join("src/rig_bindings/mylib.h");
    assert!(header.is_file(), "expected expose header");
    let text = std::fs::read_to_string(&header).unwrap();
    assert!(text.contains("RIG_NATIVE_LIB_mylib"), "{text}");
    assert!(text.contains("mylib_native"), "{text}");

    let native = dir.path().join("target/rig/mylib");
    assert!(native.is_dir(), "expected native out dir");
    let has_lib = std::fs::read_dir(&native).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("mylib_native")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(has_lib, "expected built mylib_native shared lib");
}

#[test]
fn add_path_c_empty_dir_errors_clearly() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "all:\n\t@echo ok\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.c"), "int main(void){return 0;}\n").unwrap();
    let vendor = dir.path().join("vendor/empty");
    std::fs::create_dir_all(&vendor).unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--c", &path_spec])
        .assert()
        .failure()
        .stderr(predicate::str::contains("path/git expose build failed").or(predicate::str::contains("no compilable")));
}

#[test]
fn add_path_zig_on_zig_host_builds_native() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("build.zig"),
        r#"const std = @import("std");
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const exe = b.addExecutable(.{ .name = "demo", .root_source_file = b.path("src/main.zig"), .target = target, .optimize = optimize });
    b.installArtifact(exe);
}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.zig"), "pub fn main() void {}\n").unwrap();

    let vendor = dir.path().join("vendor/zmath");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("root.zig"),
        "export fn zmath_add(a: i32, b: i32) i32 { return a + b; }\n",
    )
    .unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: zig"));

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--zig", &path_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let bindings = dir.path().join("src/rig_bindings/zmath_bindings.zig");
    assert!(bindings.is_file(), "expected zig bindings");
    let text = std::fs::read_to_string(&bindings).unwrap();
    assert!(text.contains("native_lib"), "{text}");
    assert!(text.contains("zmath_native"), "{text}");

    let native = dir.path().join("target/rig/zmath");
    assert!(native.is_dir(), "expected native out dir");
    let has_lib = std::fs::read_dir(&native).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("zmath_native")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(has_lib, "expected built zmath_native shared lib");
}
