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
        .stderr(
            predicate::str::contains("path/git expose build failed")
                .or(predicate::str::contains("no compilable")),
        );
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

#[test]
fn add_path_c_cmake_builds_native() {
    // Skip quietly when cmake is unavailable (windows CI without cmake, etc.).
    if std::process::Command::new("cmake")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skip: cmake not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "all:\n\t@echo ok\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.c"), "int main(void){return 0;}\n").unwrap();

    let vendor = dir.path().join("vendor/cmlib");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.16)\n\
         project(cmlib C)\n\
         add_library(cmlib SHARED cmlib.c)\n\
         set_target_properties(cmlib PROPERTIES OUTPUT_NAME \"cmlib_native\")\n",
    )
    .unwrap();
    std::fs::write(
        vendor.join("cmlib.h"),
        "#pragma once\nint cmlib_add(int a, int b);\n",
    )
    .unwrap();
    std::fs::write(
        vendor.join("cmlib.c"),
        "#include \"cmlib.h\"\nint cmlib_add(int a, int b) { return a + b; }\n",
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

    let header = dir.path().join("src/rig_bindings/cmlib.h");
    assert!(header.is_file(), "expected expose header");
    let text = std::fs::read_to_string(&header).unwrap();
    assert!(text.contains("RIG_NATIVE_LIB_cmlib"), "{text}");

    let native = dir.path().join("target/rig/cmlib");
    let has_lib = std::fs::read_dir(&native).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("cmlib_native")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(has_lib, "expected built cmlib_native shared lib via cmake");
}

#[test]
fn add_path_hare_missing_toolchain_is_honest() {
    // Only assert the honest error path when `hare` is absent.
    if std::process::Command::new("sh")
        .args(["-c", "command -v hare >/dev/null 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        eprintln!("skip: hare present on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "all:\n\t@echo ok\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.c"), "int main(void){return 0;}\n").unwrap();

    let vendor = dir.path().join("vendor/harelib");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("main.ha"),
        "export fn add(a: int, b: int) int = a + b;\n",
    )
    .unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--hare", &path_spec])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("hare")
                .and(predicate::str::contains("not found").or(predicate::str::contains("PATH"))),
        );
}

fn toolchain_ok(bin: &str) -> bool {
    // Odin uses `odin version` (no --version).
    let args: &[&str] = if bin == "odin" {
        &["version"]
    } else {
        &["--version"]
    };
    std::process::Command::new(bin)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_tiny_cargo_api(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        r#"[package]
name = "tiny_api"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["rlib"]
path = "src/lib.rs"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }
pub fn take_opt(p: Option<*mut u8>) -> Option<*mut u8> { p }
"#,
    )
    .unwrap();
}

#[test]
fn nim_host_cargo_facade_compiles_when_nim_present() {
    if !toolchain_ok("nim") {
        eprintln!("skip: nim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Nim host markers
    std::fs::write(
        dir.path().join("demo.nimble"),
        "version = \"0.1.0\"\nauthor = \"rig\"\ndescription = \"demo\"\nlicense = \"ISC\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.nim"), "echo \"hi\"\n").unwrap();

    let vendor = dir.path().join("vendor/tiny_api");
    write_tiny_cargo_api(&vendor);

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: nim"));

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", &path_spec, "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let binding = dir.path().join("src/rig_bindings/tiny_api.nim");
    assert!(binding.is_file(), "expected nim binding");
    let text = std::fs::read_to_string(&binding).unwrap();
    assert!(
        text.contains("tiny_api_abi_version") || text.contains("tiny_api_add"),
        "{text}"
    );
    assert!(text.contains("{.passL:"), "{text}");

    let native = dir.path().join("target/rig/tiny_api");
    let has_lib = std::fs::read_dir(&native).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("tiny_api_ffi")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(has_lib, "expected tiny_api_ffi cdylib");

    // End-to-end: compile a tiny Nim program that links the façade and calls abi_version.
    let smoke = dir.path().join("smoke_call.nim");
    std::fs::write(&smoke, "import tiny_api\necho tiny_api_abi_version()\n").unwrap();
    let out = std::process::Command::new("nim")
        .args([
            "c",
            "--hints:off",
            "--warnings:off",
            "--path:src/rig_bindings",
            "-r",
            "smoke_call.nim",
        ])
        .current_dir(dir.path())
        .output()
        .expect("run nim");
    assert!(
        out.status.success(),
        "nim compile failed:\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn v_host_cargo_facade_compiles_when_v_present() {
    if !toolchain_ok("v") {
        eprintln!("skip: v not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("v.mod"),
        "Module {\n\tname: 'demo'\n\tversion: '0.0.1'\n}\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.v"), "fn main() {}\n").unwrap();

    let vendor = dir.path().join("vendor/tiny_api");
    write_tiny_cargo_api(&vendor);

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: v"));

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", &path_spec, "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let binding = dir.path().join("src/rig_bindings/tiny_api.v");
    assert!(binding.is_file(), "expected v binding");
    let text = std::fs::read_to_string(&binding).unwrap();
    assert!(
        text.contains("fn C.tiny_api_abi_version") || text.contains("tiny_api_add"),
        "{text}"
    );
    assert!(text.contains("#flag -l"), "{text}");

    let native = dir.path().join("target/rig/tiny_api");
    let has_lib = std::fs::read_dir(&native).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("tiny_api_ffi")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(has_lib, "expected tiny_api_ffi cdylib");

    // Single-file V smoke: #include header + fn C.… decls (both required on V 0.5).
    let abs_native = dir.path().join("target/rig/tiny_api");
    let smoke = dir.path().join("smoke_call.v");
    let hdr = dir.path().join("src/rig_bindings/tiny_api_ffi.h");
    let hdr2 = abs_native.join("tiny_api_ffi.h");
    assert!(
        hdr.is_file() || hdr2.is_file(),
        "expected tiny_api_ffi.h beside bindings or native out"
    );
    let include_dir = if hdr.is_file() {
        dir.path().join("src/rig_bindings")
    } else {
        abs_native.clone()
    };
    std::fs::write(
        &smoke,
        format!(
            "#flag -L{nat}\n#flag -ltiny_api_ffi\n#flag -Wl,-rpath,{nat}\n#flag -I{inc}\n#include \"tiny_api_ffi.h\"\n\nfn C.tiny_api_abi_version() u32\n\nfn main() {{\n\tprintln(C.tiny_api_abi_version())\n}}\n",
            nat = abs_native.display(),
            inc = include_dir.display()
        ),
    )
    .unwrap();
    let out = std::process::Command::new("v")
        .args(["-keepc", "-gc", "none", "run", "smoke_call.v"])
        .current_dir(dir.path())
        .output()
        .expect("run v");
    assert!(
        out.status.success(),
        "v compile failed:\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn nim_host_path_c_emits_header_procs_when_nim_present() {
    if !toolchain_ok("nim") {
        eprintln!("skip: nim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("demo.nimble"),
        "version = \"0.1.0\"\nauthor = \"rig\"\ndescription = \"demo\"\nlicense = \"ISC\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.nim"), "echo \"hi\"\n").unwrap();

    let vendor = dir.path().join("vendor/flatlib");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("flatlib.h"),
        "#pragma once\nint flatlib_add(int a, int b);\n",
    )
    .unwrap();
    std::fs::write(
        vendor.join("flatlib.c"),
        "#include \"flatlib.h\"\nint flatlib_add(int a, int b) { return a + b; }\n",
    )
    .unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: nim"));

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--c", &path_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let binding = dir.path().join("src/rig_bindings/flatlib.nim");
    assert!(binding.is_file(), "expected nim path binding");
    let text = std::fs::read_to_string(&binding).unwrap();
    assert!(
        text.contains("proc flatlib_add*"),
        "expected discovered C prototype in nim binder:\n{text}"
    );
}

#[test]
fn odin_host_path_c_emits_header_procs_when_odin_present() {
    if !toolchain_ok("odin") {
        eprintln!("skip: odin not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ols.json"), "{}\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/main.odin"),
        "package main\nmain :: proc() {}\n",
    )
    .unwrap();

    let vendor = dir.path().join("vendor/flatlib");
    std::fs::create_dir_all(&vendor).unwrap();
    std::fs::write(
        vendor.join("flatlib.h"),
        "#pragma once\nint flatlib_add(int a, int b);\n",
    )
    .unwrap();
    std::fs::write(
        vendor.join("flatlib.c"),
        "#include \"flatlib.h\"\nint flatlib_add(int a, int b) { return a + b; }\n",
    )
    .unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: odin"));

    let path_spec = format!("path:{}", vendor.display());
    rig()
        .current_dir(dir.path())
        .args(["add", "--c", &path_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let binding = dir.path().join("src/rig_bindings/flatlib.odin");
    assert!(binding.is_file(), "expected odin path binding");
    let text = std::fs::read_to_string(&binding).unwrap();
    assert!(
        text.contains("flatlib_add"),
        "expected discovered C prototype in odin binder:\n{text}"
    );
    assert!(text.contains("foreign rig_lib"), "{text}");
}
