//! Exposed dependencies must be really linked (or rejected), never string stubs.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

fn rig() -> Command {
    Command::cargo_bin("rig").unwrap()
}

fn have(bin: &str) -> bool {
    std::process::Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_flat_c_lib(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("flatlib.h"),
        "#pragma once\nint flatlib_add(int a, int b);\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("flatlib.c"),
        "#include \"flatlib.h\"\nint flatlib_add(int a, int b) { return a + b; }\n",
    )
    .unwrap();
}

fn write_rig_toml(root: &Path, host: &str, deps: &str) {
    std::fs::write(
        root.join("rig.toml"),
        format!(
            "schema_version = 1\n\n[host]\nlanguage = \"{host}\"\n\n{deps}\n[expose]\nenabled = true\ndir = \"src/rig_bindings\"\ncache = \".rig/cache\"\nbuild_dir = \"target/rig\"\n"
        ),
    )
    .unwrap();
}

#[test]
fn rust_host_path_c_dep_links_and_calls_native_code() {
    if !have("cc") {
        eprintln!("skip: cc missing");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"sum={}\", unsafe { rig_bindings::flatlib::flatlib_add(2, 40) }); }\n",
    )
    .unwrap();
    write_flat_c_lib(&root.join("vendor/flatlib"));

    rig().current_dir(root).arg("init").assert().success();
    rig()
        .current_dir(root)
        .args(["add", "--c", "path:vendor/flatlib"])
        .assert()
        .success();

    let binding = std::fs::read_to_string(root.join("src/rig_bindings/flatlib.rs")).unwrap();
    assert!(
        !binding.contains("RIG_LOAD_PATH"),
        "string stub:\n{binding}"
    );
    assert!(
        binding.contains("pub fn flatlib_add(a: i32, b: i32) -> i32;"),
        "{binding}"
    );

    let run = std::process::Command::new("cargo")
        .args(["run", "-q", "-j2"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "host build/run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "sum=42");

    // Removing the dep removes the link wiring too.
    rig()
        .current_dir(root)
        .args(["rm", "flatlib"])
        .assert()
        .success();
    let build_rs = std::fs::read_to_string(root.join("build.rs")).unwrap_or_default();
    assert!(!build_rs.contains("flatlib"), "{build_rs}");
}

#[test]
fn hosts_reject_path_deps_they_cannot_link_instead_of_writing_stubs() {
    for (marker, marker_body, host, out) in [
        ("Makefile", "all:\n", "c", "src/rig_bindings/dlib.h"),
        (
            "demo.nimble",
            "version = \"0.1.0\"\n",
            "nim",
            "src/rig_bindings/dlib.nim",
        ),
        (
            "Cargo.toml",
            "[package]\nname=\"h\"\nversion=\"0.1.0\"\n",
            "rust",
            "src/rig_bindings/dlib.rs",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(marker), marker_body).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("vendor/dlib")).unwrap();
        std::fs::write(root.join("vendor/dlib/dlib.d"), "module dlib;\n").unwrap();
        rig().current_dir(root).arg("init").assert().success();
        rig()
            .current_dir(root)
            .args(["add", "--d", "path:vendor/dlib"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("dlib"));
        assert!(!root.join(out).exists(), "{host} host wrote a stub binder");
        let manifest = std::fs::read_to_string(root.join("rig.toml")).unwrap();
        assert!(
            !manifest.contains("dlib"),
            "{host}: failed add was pinned:\n{manifest}"
        );
    }
}

#[test]
fn csharp_host_registry_dep_becomes_a_package_reference() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("App.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n  </PropertyGroup>\n</Project>\n",
    )
    .unwrap();
    write_rig_toml(
        root,
        "csharp",
        "[dependencies.\"Newtonsoft.Json\"]\necosystem = \"csharp\"\nversion = \"13.0.3\"\nexpose = true\n",
    );
    rig().current_dir(root).arg("sync").assert().success();
    let csproj = std::fs::read_to_string(root.join("App.csproj")).unwrap();
    assert!(
        csproj.contains("<PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.3\" />"),
        "{csproj}"
    );
    assert!(csproj.trim_end().ends_with("</Project>"), "{csproj}");
    // Idempotent.
    rig().current_dir(root).arg("sync").assert().success();
    let again = std::fs::read_to_string(root.join("App.csproj")).unwrap();
    assert_eq!(again.matches("PackageReference").count(), 1, "{again}");

    rig()
        .current_dir(root)
        .args(["rm", "Newtonsoft.Json"])
        .assert()
        .success();
    let removed = std::fs::read_to_string(root.join("App.csproj")).unwrap();
    assert!(!removed.contains("Newtonsoft.Json"), "{removed}");
}

#[test]
fn d_host_registry_dep_becomes_a_dub_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dub.json"),
        "{\n  \"name\": \"demo\",\n  \"targetType\": \"executable\"\n}\n",
    )
    .unwrap();
    write_rig_toml(
        root,
        "d",
        "[dependencies.vibe-d]\necosystem = \"d\"\nversion = \"0.10.1\"\nexpose = true\n",
    );
    rig().current_dir(root).arg("sync").assert().success();
    let dub: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("dub.json")).unwrap()).unwrap();
    assert_eq!(dub["dependencies"]["vibe-d"], "0.10.1", "{dub}");
    assert_eq!(dub["name"], "demo");

    rig()
        .current_dir(root)
        .args(["rm", "vibe-d"])
        .assert()
        .success();
    let dub: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("dub.json")).unwrap()).unwrap();
    assert!(dub["dependencies"].get("vibe-d").is_none(), "{dub}");
}

#[test]
fn d_host_registry_dep_in_dub_sdl() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("dub.sdl"),
        "name \"demo\"\ntargetType \"executable\"\n",
    )
    .unwrap();
    write_rig_toml(
        root,
        "d",
        "[dependencies.mir-core]\necosystem = \"d\"\nversion = \"1.7.1\"\nexpose = true\n",
    );
    rig().current_dir(root).arg("sync").assert().success();
    let sdl = std::fs::read_to_string(root.join("dub.sdl")).unwrap();
    assert!(
        sdl.contains("dependency \"mir-core\" version=\"1.7.1\""),
        "{sdl}"
    );
    assert!(sdl.contains("name \"demo\""), "{sdl}");
}

#[test]
fn facade_turns_a_panic_into_a_fallback_value_instead_of_aborting() {
    if !have("cc") {
        eprintln!("skip: cc missing");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Makefile"), "all:\n").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let boom = root.join("vendor/boom");
    std::fs::create_dir_all(boom.join("src")).unwrap();
    std::fs::write(
        boom.join("Cargo.toml"),
        "[package]\nname = \"boom\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        boom.join("src/lib.rs"),
        "pub fn div(a: i32, b: i32) -> i32 { if b == 0 { panic!(\"divide by zero\") } a / b }\n",
    )
    .unwrap();

    rig().current_dir(root).arg("init").assert().success();
    rig()
        .current_dir(root)
        .args(["add", "--rust", "path:vendor/boom"])
        .assert()
        .success();

    let main_c = root.join("main.c");
    std::fs::write(
        &main_c,
        concat!(
            "#include <stdio.h>\n#include \"boom_ffi.h\"\n",
            "int main(void) {\n",
            "  int ok = boom_div(84, 2);\n",
            "  const char *after_ok = boom_last_panic();\n",
            "  int bad = boom_div(1, 0);\n",
            "  const char *after_bad = boom_last_panic();\n",
            "  printf(\"%d %d|%s|%s\\n\", ok, bad, after_ok ? after_ok : \"(null)\", after_bad ? after_bad : \"(null)\");\n",
            "  return 0;\n}\n"
        ),
    )
    .unwrap();
    let native = root.join("target/rig/boom");
    let exe = root.join("smoke");
    let cc = std::process::Command::new("cc")
        .arg(&main_c)
        .arg(format!("-I{}", root.join("src/rig_bindings").display()))
        .arg(format!("-L{}", native.display()))
        .arg("-lboom_ffi")
        .arg(format!("-Wl,-rpath,{}", native.display()))
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        cc.status.success(),
        "{}",
        String::from_utf8_lossy(&cc.stderr)
    );
    let run = std::process::Command::new(&exe).output().unwrap();
    assert!(
        run.status.success(),
        "panic crossed the C boundary (status {:?}):\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    // 0 alone is ambiguous with a rejected input; `_last_panic` disambiguates.
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "42 0|(null)|divide by zero"
    );
}
