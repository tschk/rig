//! Pass D/E: add → sync → remove correctness for cargo façades (C + Rust hosts).
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;

fn rig() -> Command {
    Command::cargo_bin("rig").unwrap()
}

fn write_c_host(dir: &Path) {
    fs::write(dir.join("Makefile"), "all:\n\t@echo ok\n").unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.c"), "int main(void){return 0;}\n").unwrap();
}

#[test]
fn add_sync_remove_cargo_facade_on_c_host() {
    let dir = tempfile::tempdir().unwrap();
    write_c_host(dir.path());

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: c"));

    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", "sha2@0.10.9", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"))
        .stdout(predicate::str::contains("sha2.h"));

    let manifest = fs::read_to_string(dir.path().join("rig.toml")).unwrap();
    assert!(manifest.contains("sha2"), "{manifest}");
    assert!(
        dir.path().join("src/rig_bindings/sha2.h").is_file(),
        "expected C binding header"
    );
    assert!(
        dir.path().join(".rig/shims/sha2/src/lib.rs").is_file(),
        "expected façade sources"
    );
    let dylib = dir.path().join("target/rig/sha2");
    assert!(dylib.is_dir(), "expected native out dir");
    let has_lib = fs::read_dir(&dylib).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("sha2_ffi")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(
        has_lib,
        "expected installed sha2_ffi cdylib under {}",
        dylib.display()
    );

    // sync is idempotent
    rig()
        .current_dir(dir.path())
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("synced"));

    // remove cleans expose + shim
    rig()
        .current_dir(dir.path())
        .args(["rm", "sha2", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    let manifest = fs::read_to_string(dir.path().join("rig.toml")).unwrap();
    assert!(!manifest.contains("[dependencies.sha2]"), "{manifest}");
    assert!(
        !dir.path().join("src/rig_bindings/sha2.h").exists(),
        "binding header should be gone"
    );
    assert!(
        !dir.path().join(".rig/shims/sha2").exists(),
        "shim dir should be gone"
    );
}

#[test]
fn rust_host_add_reexports_without_cdylib() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"demo\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "").unwrap();

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();

    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", "sha2@0.10.9", "-y"])
        .assert()
        .success();

    let reexport = dir.path().join("src/rig_bindings/sha2.rs");
    assert!(reexport.is_file());
    let body = fs::read_to_string(&reexport).unwrap();
    assert!(body.contains("pub use sha2::*"), "{body}");

    let cargo = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(cargo.contains("sha2"), "{cargo}");
    assert!(
        !dir.path().join(".rig/shims/sha2").exists(),
        "rust←cargo should not build cdylib façade"
    );

    rig()
        .current_dir(dir.path())
        .args(["ui", "sha2", "-y"])
        .assert()
        .success();
    let cargo = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(
        !cargo.contains("sha2"),
        "cargo dep should be removed: {cargo}"
    );
}

#[test]
fn upgrade_empty_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    write_c_host(dir.path());
    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();
    rig()
        .current_dir(dir.path())
        .args(["up"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to upgrade"));
}

#[test]
fn base64_enrichment_facade_builds_on_c_host() {
    let dir = tempfile::tempdir().unwrap();
    write_c_host(dir.path());

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: c"));

    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", "base64@0.22.1", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));

    let host_hdr = fs::read_to_string(dir.path().join("src/rig_bindings/base64.h")).unwrap();
    assert!(
        host_hdr.contains("base64_ffi.h") || host_hdr.contains("base64_encode"),
        "{host_hdr}"
    );
    let ffi_hdr = dir.path().join("src/rig_bindings/base64_ffi.h");
    let ffi_hdr2 = dir.path().join(".rig/shims/base64/base64_ffi.h");
    let hdr_path = if ffi_hdr.is_file() { ffi_hdr } else { ffi_hdr2 };
    let hdr = fs::read_to_string(&hdr_path).unwrap();
    assert!(hdr.contains("base64_encode"), "{hdr}");
    assert!(hdr.contains("base64_decode"), "{hdr}");
    let lib = dir.path().join(".rig/shims/base64/src/lib.rs");
    let lib_txt = fs::read_to_string(&lib).unwrap();
    assert!(lib_txt.contains("fn base64_encode"), "{lib_txt}");
    let native = dir.path().join("target/rig/base64");
    let has_lib = fs::read_dir(&native).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.contains("base64_ffi")
            && (n.ends_with(".dylib") || n.ends_with(".so") || n.ends_with(".dll"))
    });
    assert!(has_lib, "expected base64_ffi cdylib");
}
