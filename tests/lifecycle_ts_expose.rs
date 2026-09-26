//! TypeScript hosts: detection, scriptc bindings, merged `--ffi` manifest, and
//! an end-to-end call into a Rust crate and a local C library.
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;

fn rig() -> Command {
    Command::cargo_bin("rig").unwrap()
}

fn write_ts_host(dir: &Path) {
    fs::write(
        dir.join("tsconfig.json"),
        "{\n  \"compilerOptions\": { \"strict\": true, \"noEmit\": true }\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("package.json"),
        "{\n  \"name\": \"ts-demo\",\n  \"private\": true,\n  \"type\": \"module\"\n}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
}

fn write_c_dep(dir: &Path) {
    let native = dir.join("cmath");
    fs::create_dir_all(&native).unwrap();
    fs::write(
        native.join("math.h"),
        "/* local C dependency */\n#include <stdint.h>\nint32_t cmath_double(int32_t value);\nconst char *cmath_name(void);\n",
    )
    .unwrap();
    fs::write(
        native.join("math.c"),
        "#include \"math.h\"\nint32_t cmath_double(int32_t value) { return value * 2; }\nconst char *cmath_name(void) { return \"cmath\"; }\n",
    )
    .unwrap();
}

#[test]
fn add_sync_remove_on_typescript_host() {
    let dir = tempfile::tempdir().unwrap();
    write_ts_host(dir.path());
    write_c_dep(dir.path());

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("host: typescript"));

    let manifest = fs::read_to_string(dir.path().join("rig.toml")).unwrap();
    assert!(manifest.contains("name = \"ts-demo\""), "{manifest}");

    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", "crc32fast@1.4.2", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"))
        .stdout(predicate::str::contains("crc32fast.ts"));

    rig()
        .current_dir(dir.path())
        .args(["add", "--c", "path:cmath", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cmath.ts"));

    let bindings = dir.path().join("src/rig_bindings");
    let rust_module = fs::read_to_string(bindings.join("crc32fast.ts")).unwrap();
    assert!(
        rust_module.contains("export declare function crc32fast_hash(data: Uint8Array): number;"),
        "{rust_module}"
    );
    let c_module = fs::read_to_string(bindings.join("cmath.ts")).unwrap();
    assert!(
        c_module.contains("export declare function cmath_double(value: number): number;"),
        "{c_module}"
    );
    // The C library's `const char *` return has no scriptc class, so the
    // declaration is left out rather than mistyped.
    assert!(!c_module.contains("cmath_name"), "{c_module}");

    // One manifest per dependency, merged into the one scriptc accepts.
    let merged: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bindings.join("rig.ffi.json")).unwrap()).unwrap();
    let names: Vec<&str> = merged["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"crc32fast_hash"), "{names:?}");
    assert!(names.contains(&"cmath_double"), "{names:?}");
    assert!(!names.contains(&"cmath_name"), "{names:?}");
    assert_eq!(merged["libraries"].as_array().unwrap().len(), 2);
    assert!(merged["libraries"][0].as_str().unwrap().starts_with('/'));

    // sync is idempotent
    rig()
        .current_dir(dir.path())
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("synced 2 expose artifact(s)"));

    // remove cleans the module, its manifest and its slot in the merged one
    rig()
        .current_dir(dir.path())
        .args(["rm", "cmath", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
    assert!(!bindings.join("cmath.ts").exists());
    assert!(!bindings.join("cmath.ffi.json").exists());
    assert!(!bindings.join("cmath.h").exists());
    let merged: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bindings.join("rig.ffi.json")).unwrap()).unwrap();
    assert_eq!(merged["functions"].as_array().unwrap().len(), 2);
    assert_eq!(merged["libraries"].as_array().unwrap().len(), 1);
}

#[test]
fn typescript_host_runs_native_calls_through_scriptc() {
    if which("scriptc").is_none() {
        eprintln!("skipping: scriptc is not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    write_ts_host(dir.path());
    write_c_dep(dir.path());

    rig()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success();
    rig()
        .current_dir(dir.path())
        .args(["add", "--rust", "crc32fast@1.4.2", "-y"])
        .assert()
        .success();
    rig()
        .current_dir(dir.path())
        .args(["add", "--c", "path:cmath", "-y"])
        .assert()
        .success();

    fs::write(
        dir.path().join("src/main.ts"),
        "import { crc32fast_hash } from \"./rig_bindings/crc32fast\";\n\
         import { cmath_double } from \"./rig_bindings/cmath\";\n\
         \n\
         const crc = crc32fast_hash(new TextEncoder().encode(\"hello\"));\n\
         console.log(\"crc32\", crc, \"double\", cmath_double(21));\n\
         if (crc !== 907060870 || cmath_double(21) !== 42) {\n\
         \x20 throw new Error(\"native call returned an unexpected value\");\n\
         }\n",
    )
    .unwrap();

    rig()
        .current_dir(dir.path())
        .args(["build", "--", "src/main.ts", "-o", "host"])
        .assert()
        .success();

    let out = Command::new(dir.path().join("host")).output().unwrap();
    assert!(
        out.status.success(),
        "host failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("crc32 907060870"), "{stdout}");
    assert!(stdout.contains("double 42"), "{stdout}");
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .find(|candidate| candidate.is_file())
    })
}
