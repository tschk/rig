//! Generic C ABI / cdylib façade generation for arbitrary cargo crates.
//!
//! Most crates.io packages are `rlib`-only (and may `#![forbid(unsafe_code)]`),
//! so non-Rust hosts cannot link them directly. For any `rig add --rust <crate>`,
//! rig materializes `.rig/shims/<pkg>/` — a thin `cdylib` that exports a
//! discoverable surface:
//!
//! - `{crate}_abi_version() -> u32`
//! - `{crate}_version() -> *const c_char`
//! - `{crate}_name() -> *const c_char`
//!
//! Strategy:
//! 1. If the dependency already declares `crate-type` including `cdylib`
//!    (inspectable via path), build that crate directly — no redundant façade.
//! 2. Else generate a thin façade crate (unsafe only in the façade) that depends
//!    on the package and exports the markers above.
//! 3. Known enrichments (e.g. `rx4`) may export additional symbols.
//!
//! Honest limits: arbitrary Rust APIs (generics, traits, async, non-`repr(C)`
//! types) are **not** auto-wrapped. Full cbindgen of a public API is not
//! automatic without FFI-safe annotations.

use crate::manifest::Dependency;
use crate::resolve::ResolvedPackage;
use crate::util::AppCtx;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub struct ShimArtifacts {
    pub shim_dir: PathBuf,
    pub header: PathBuf,
    pub lib_name: String,
    pub out_dir: PathBuf,
    /// When true, `shim_dir` is the upstream crate itself (already a cdylib).
    pub passthrough: bool,
}

pub fn shim_dir(ctx: &AppCtx, name: &str) -> PathBuf {
    ctx.root.join(".rig/shims").join(name)
}

pub fn native_out_dir(ctx: &AppCtx, name: &str) -> PathBuf {
    let native = ctx
        .manifest
        .dependencies
        .get(name)
        .and_then(|d| d.expose_opts.as_ref())
        .and_then(|o| o.native.clone())
        .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
    ctx.root.join(native)
}

fn crate_ident(name: &str) -> String {
    name.replace('-', "_")
}

/// Parse a Cargo.toml and report whether its `[lib]` crate-type includes `cdylib`.
pub fn crate_has_cdylib(cargo_toml: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(cargo_toml) else {
        return false;
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return false;
    };
    let Some(lib) = value.get("lib") else {
        return false;
    };
    match lib.get("crate-type") {
        Some(toml::Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some("cdylib")),
        Some(toml::Value::String(s)) => s == "cdylib",
        _ => false,
    }
}

fn lib_name_from_cargo(cargo_toml: &Path, fallback: &str) -> String {
    let Ok(text) = std::fs::read_to_string(cargo_toml) else {
        return crate_ident(fallback);
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return crate_ident(fallback);
    };
    if let Some(name) = value
        .get("lib")
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
    {
        return name.to_string();
    }
    if let Some(name) = value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
    {
        return crate_ident(name);
    }
    crate_ident(fallback)
}

fn resolve_dep_path(
    name: &str,
    resolved: Option<&ResolvedPackage>,
    dep: &Dependency,
) -> Option<PathBuf> {
    if let Some(path) = resolved
        .and_then(|r| r.path.as_deref())
        .or(dep.path.as_deref())
    {
        let pb = PathBuf::from(path);
        if pb.join("Cargo.toml").is_file() {
            return Some(pb);
        }
    }
    let crate_name = if name == "rotary" { "rx4" } else { name };
    if crate_name == "rx4" {
        return find_local_rotary();
    }
    None
}

/// Ensure a façade (or passthrough) exists for `name`. Returns paths for native build.
pub fn ensure_shim(
    ctx: &AppCtx,
    name: &str,
    resolved: Option<&ResolvedPackage>,
    dep: &Dependency,
) -> Result<ShimArtifacts> {
    let out_dir = native_out_dir(ctx, name);
    std::fs::create_dir_all(&out_dir)?;

    // Passthrough: upstream already ships a cdylib — build it directly.
    if let Some(path) = resolve_dep_path(name, resolved, dep) {
        let manifest = path.join("Cargo.toml");
        if crate_has_cdylib(&manifest) {
            let lib_name = lib_name_from_cargo(&manifest, name);
            let header_path = write_passthrough_header(&out_dir, name, &lib_name)?;
            // Also keep a copy under .rig/shims for discoverability.
            let dir = shim_dir(ctx, name);
            std::fs::create_dir_all(&dir)?;
            let _ = std::fs::copy(&header_path, dir.join(header_path.file_name().unwrap()));
            std::fs::write(
                dir.join("PASSTHROUGH"),
                format!("upstream cdylib at {}\n", path.display()),
            )?;
            return Ok(ShimArtifacts {
                shim_dir: path,
                header: header_path,
                lib_name,
                out_dir,
                passthrough: true,
            });
        }
    }

    let dir = shim_dir(ctx, name);
    std::fs::create_dir_all(dir.join("src")).with_context(|| format!("mkdir {}", dir.display()))?;

    let lib_name = format!("{}_ffi", crate_ident(name));
    let version = resolved
        .map(|r| r.version.as_str())
        .or(dep.version.as_deref())
        .unwrap_or("*");

    let dep_toml = cargo_dep_line(name, resolved, dep)?;
    let cargo_toml = format!(
        r#"[package]
name = "{lib_name}"
version = "0.1.0"
edition = "2021"
publish = false
license = "ISC"
description = "rig-generated C ABI façade for {name}"

[lib]
name = "{lib_name}"
crate-type = ["cdylib", "rlib"]
path = "src/lib.rs"

[dependencies]
{dep_toml}
"#
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml)?;

    let lib_rs = match name {
        "rx4" | "rotary" => rx4_lib_rs(),
        "sha2" => sha2_lib_rs(version),
        _ => generic_lib_rs(name, version),
    };
    std::fs::write(dir.join("src/lib.rs"), lib_rs)?;

    let header = match name {
        "rx4" | "rotary" => rx4_header(),
        "sha2" => sha2_header(),
        _ => generic_header(name, &lib_name),
    };
    let header_path = dir.join(format!("{lib_name}.h"));
    std::fs::write(&header_path, header)?;

    Ok(ShimArtifacts {
        shim_dir: dir,
        header: header_path,
        lib_name,
        out_dir,
        passthrough: false,
    })
}

fn write_passthrough_header(out_dir: &Path, name: &str, lib_name: &str) -> Result<PathBuf> {
    // Markers still emitted so hosts have a stable discovery surface even when
    // linking an upstream cdylib that may export a different API.
    let header = generic_header(name, lib_name);
    // Note: passthrough does not inject these symbols into the upstream dylib;
    // the header documents the *façade* convention. Hosts should prefer the
    // upstream's own headers when present. We still write a discovery header.
    let path = out_dir.join(format!("{}_rig_discover.h", crate_ident(name)));
    std::fs::write(
        &path,
        format!(
            "/* rig passthrough: `{name}` already ships cdylib `{lib_name}`.\n\
         * Prefer upstream headers. Discovery markers below are NOT injected\n\
         * into the upstream dylib — link upstream symbols directly.\n\
         */\n{header}"
        ),
    )?;
    // Prefer any existing .h next to the crate if we can find one later; for now
    // return discovery header.
    Ok(path)
}

fn cargo_dep_line(
    name: &str,
    resolved: Option<&ResolvedPackage>,
    dep: &Dependency,
) -> Result<String> {
    let crate_name = if name == "rotary" { "rx4" } else { name };

    if let Some(path) = resolved
        .and_then(|r| r.path.as_deref())
        .or(dep.path.as_deref())
    {
        return Ok(format!(
            "{crate_name} = {{ path = \"{}\", default-features = false }}",
            escape_toml_str(path)
        ));
    }
    if let Some(git) = resolved
        .and_then(|r| r.git.as_deref())
        .or(dep.git.as_deref())
    {
        return Ok(format!(
            "{crate_name} = {{ git = \"{}\", default-features = false }}",
            escape_toml_str(git)
        ));
    }

    if crate_name == "rx4"
        && let Some(local) = find_local_rotary()
    {
        return Ok(format!(
            "rx4 = {{ path = \"{}\", default-features = false }}",
            escape_toml_str(&local.display().to_string())
        ));
    }

    let ver = resolved
        .map(|r| r.version.as_str())
        .or(dep.version.as_deref())
        .unwrap_or("*");
    if ver == "*" || ver == "git" || ver == "path" {
        bail!("cannot pin {crate_name} for shim: missing concrete version/path/git");
    }
    Ok(format!(
        "{crate_name} = {{ version = \"{}\", default-features = false }}",
        escape_toml_str(ver)
    ))
}

fn find_local_rotary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RIG_RX4_PATH") {
        let pb = PathBuf::from(p);
        if pb.join("Cargo.toml").is_file() {
            return Some(pb);
        }
    }
    let home = dirs::home_dir()?;
    let cand = home.join("projects/rotary");
    if cand.join("Cargo.toml").is_file() {
        return Some(cand);
    }
    None
}

fn escape_toml_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn rx4_lib_rs() -> String {
    r#"//! rig-generated C ABI façade for rx4 (rotary).
//! Narrow surface for polyglot hosts — not a full Agent loop.
//!
//! ABI 2 adds opaque agent handle stubs + prompt smoke (link/call proof).
//! These do **not** yet drive `rx4::Agent::prompt`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::OnceLock;

/// Opaque host-facing agent handle (stub — not a live `rx4::Agent`).
pub struct Rx4Agent {
    _alive: u8,
}

/// ABI revision for this façade (bump when symbols/semantics change).
#[no_mangle]
pub extern "C" fn rx4_abi_version() -> u32 {
    2
}

/// Null-terminated `rx4::VERSION` from the linked rx4 crate.
#[no_mangle]
pub extern "C" fn rx4_version() -> *const c_char {
    static V: OnceLock<CString> = OnceLock::new();
    V.get_or_init(|| CString::new(rx4::VERSION).expect("rx4::VERSION has interior NUL"))
        .as_ptr()
}

/// Null-terminated package name.
#[no_mangle]
pub extern "C" fn rx4_name() -> *const c_char {
    b"rx4\0".as_ptr() as *const c_char
}

/// Allocate an opaque agent stub. Host must call `rx4_agent_free`.
#[no_mangle]
pub extern "C" fn rx4_agent_new() -> *mut Rx4Agent {
    Box::into_raw(Box::new(Rx4Agent { _alive: 1 }))
}

/// Free a handle from `rx4_agent_new`. No-op on null.
#[no_mangle]
pub extern "C" fn rx4_agent_free(agent: *mut Rx4Agent) {
    if agent.is_null() {
        return;
    }
    // SAFETY: only pointers from `rx4_agent_new` are valid.
    unsafe {
        drop(Box::from_raw(agent));
    }
}

/// Link/call smoke: validate handle + UTF-8 prompt without running the agent loop.
///
/// Returns:
/// - `0` success
/// - `-1` null agent
/// - `-2` null prompt
/// - `-3` empty prompt
/// - `-4` prompt not valid UTF-8
#[no_mangle]
pub extern "C" fn rx4_prompt_smoke(agent: *mut Rx4Agent, prompt: *const c_char) -> i32 {
    if agent.is_null() {
        return -1;
    }
    if prompt.is_null() {
        return -2;
    }
    // SAFETY: caller passes a NUL-terminated C string (or null, handled above).
    let cstr = unsafe { CStr::from_ptr(prompt) };
    match cstr.to_str() {
        Ok(s) if s.is_empty() => -3,
        Ok(_) => 0,
        Err(_) => -4,
    }
}
"#
    .into()
}

fn rx4_header() -> String {
    r#"/* Auto-generated by rig — C ABI façade for rx4 */
#ifndef RIG_RX4_FFI_H
#define RIG_RX4_FFI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct Rx4Agent Rx4Agent;

/** Façade ABI revision (not the rx4 crate semver). Currently 2. */
uint32_t rx4_abi_version(void);

/** Null-terminated rx4 crate version string (static storage). */
const char *rx4_version(void);

/** Null-terminated package name. */
const char *rx4_name(void);

/** Allocate opaque agent stub (not a full rx4::Agent). Free with rx4_agent_free. */
Rx4Agent *rx4_agent_new(void);

/** Free handle from rx4_agent_new. Null-safe. */
void rx4_agent_free(Rx4Agent *agent);

/** Smoke: prove link+call. 0 ok; -1 null agent; -2 null prompt; -3 empty; -4 bad utf8. */
int32_t rx4_prompt_smoke(Rx4Agent *agent, const char *prompt);

#ifdef __cplusplus
}
#endif

#endif /* RIG_RX4_FFI_H */
"#
    .into()
}

fn sha2_lib_rs(version: &str) -> String {
    format!(
        r#"//! rig-generated C ABI façade for `sha2` (markers + SHA-256 helper).
//! Domain helper is a known enrichment — not a full digest API.

use sha2::{{Digest, Sha256}};
use std::os::raw::c_char;
use std::slice;

#[no_mangle]
pub extern "C" fn sha2_abi_version() -> u32 {{
    2
}}

#[no_mangle]
pub extern "C" fn sha2_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

#[no_mangle]
pub extern "C" fn sha2_name() -> *const c_char {{
    b"sha2\0".as_ptr() as *const c_char
}}

/// Hash `len` bytes at `data` into a 32-byte `out` buffer.
///
/// Returns `0` on success, `-1` if `data` is null with nonzero len, `-2` if `out` is null.
#[no_mangle]
pub extern "C" fn sha2_hash_256(data: *const u8, len: usize, out: *mut u8) -> i32 {{
    if out.is_null() {{
        return -2;
    }}
    if len > 0 && data.is_null() {{
        return -1;
    }}
    // SAFETY: caller provides `len` readable bytes (or len==0) and 32 writable out bytes.
    let input = if len == 0 {{
        &[][..]
    }} else {{
        unsafe {{ slice::from_raw_parts(data, len) }}
    }};
    let digest = Sha256::digest(input);
    unsafe {{
        slice::from_raw_parts_mut(out, 32).copy_from_slice(&digest);
    }}
    0
}}
"#
    )
}

fn sha2_header() -> String {
    r#"/* Auto-generated by rig — C ABI façade for sha2 (ABI 2) */
#ifndef RIG_SHA2_FFI_H
#define RIG_SHA2_FFI_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
uint32_t sha2_abi_version(void);
const char *sha2_version(void);
const char *sha2_name(void);
/** SHA-256: write 32 bytes to out. 0 ok; -1 null data; -2 null out. */
int32_t sha2_hash_256(const uint8_t *data, size_t len, uint8_t *out);
#ifdef __cplusplus
}
#endif
#endif /* RIG_SHA2_FFI_H */
"#
    .into()
}

/// Generate generic façade `lib.rs` for an arbitrary cargo package.
pub fn generic_lib_rs(name: &str, version: &str) -> String {
    let safe = crate_ident(name);
    format!(
        r#"//! rig-generated C ABI façade for `{name}`.
//! Discoverable markers only — arbitrary Rust APIs are not auto-exported.
//! This façade crate may contain `unsafe` even when `{name}` forbids it.

use std::os::raw::c_char;

// Pull the dependency into the link graph (markers do not call into it yet).
#[allow(unused_imports)]
use {safe} as _;

/// Façade ABI revision (bump when symbols/semantics change).
#[no_mangle]
pub extern "C" fn {safe}_abi_version() -> u32 {{
    1
}}

/// Null-terminated version string pinned by rig at generate time.
#[no_mangle]
pub extern "C" fn {safe}_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

/// Null-terminated crate name.
#[no_mangle]
pub extern "C" fn {safe}_name() -> *const c_char {{
    concat!("{name}", "\0").as_ptr() as *const c_char
}}
"#
    )
}

/// Generate generic C header matching [`generic_lib_rs`].
pub fn generic_header(name: &str, lib_name: &str) -> String {
    let safe = crate_ident(name);
    format!(
        r#"/* Auto-generated by rig — C ABI façade for {name} ({lib_name}) */
#ifndef RIG_{guard}_FFI_H
#define RIG_{guard}_FFI_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {{
#endif
/** Façade ABI revision (not necessarily the crate semver). */
uint32_t {safe}_abi_version(void);
/** Null-terminated version string (static storage). */
const char *{safe}_version(void);
/** Null-terminated crate name (static storage). */
const char *{safe}_name(void);
#ifdef __cplusplus
}}
#endif
#endif /* RIG_{guard}_FFI_H */
"#,
        guard = safe.to_uppercase()
    )
}

/// Copy built cdylib (+ header) into `out_dir`.
pub fn install_artifacts(artifacts: &ShimArtifacts, target_dir: &Path) -> Result<PathBuf> {
    let lib_stem = format!("lib{}", artifacts.lib_name);
    let mut found: Option<PathBuf> = None;
    for (dir, name) in [
        (target_dir.join("release"), format!("{lib_stem}.dylib")),
        (target_dir.join("release"), format!("{lib_stem}.so")),
        (target_dir.join("release"), format!("{lib_stem}.dll")),
        (target_dir.join("debug"), format!("{lib_stem}.dylib")),
        (target_dir.join("debug"), format!("{lib_stem}.so")),
        (target_dir.join("debug"), format!("{lib_stem}.dll")),
        (
            target_dir.join("release"),
            format!("{}.dll", artifacts.lib_name),
        ),
        (
            target_dir.join("debug"),
            format!("{}.dll", artifacts.lib_name),
        ),
    ] {
        let p = dir.join(&name);
        if p.is_file() {
            found = Some(p);
            break;
        }
    }
    let src = found.with_context(|| {
        format!(
            "cdylib for {} not found under {}",
            artifacts.lib_name,
            target_dir.display()
        )
    })?;
    let dest = artifacts.out_dir.join(src.file_name().unwrap());
    std::fs::copy(&src, &dest)
        .with_context(|| format!("copy {} → {}", src.display(), dest.display()))?;
    if artifacts.header.is_file() {
        let hdr_dest = artifacts
            .out_dir
            .join(artifacts.header.file_name().unwrap());
        std::fs::copy(&artifacts.header, &hdr_dest)?;
        if let Some(pkg) = artifacts.shim_dir.file_name() {
            let _ = std::fs::copy(
                &artifacts.header,
                artifacts
                    .out_dir
                    .join(format!("{}.h", pkg.to_string_lossy())),
            );
        }
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_symbols_use_crate_prefix() {
        let lib = generic_lib_rs("sha2", "0.10.9");
        assert!(lib.contains("fn sha2_abi_version"));
        assert!(lib.contains("fn sha2_version"));
        assert!(lib.contains("fn sha2_name"));
        assert!(!lib.contains("_rig_version"));
        assert!(lib.contains("use sha2 as _"));
        let hdr = generic_header("sha2", "sha2_ffi");
        assert!(hdr.contains("sha2_abi_version"));
        assert!(hdr.contains("sha2_name"));
    }

    #[test]
    fn hyphenated_crate_idents() {
        let lib = generic_lib_rs("crypto-common", "0.1.0");
        assert!(lib.contains("fn crypto_common_abi_version"));
        assert!(lib.contains("use crypto_common as _"));
    }

    #[test]
    fn rx4_enrichment_keeps_smoke_and_markers() {
        let lib = rx4_lib_rs();
        assert!(lib.contains("fn rx4_abi_version"));
        assert!(lib.contains("fn rx4_version"));
        assert!(lib.contains("fn rx4_name"));
        assert!(lib.contains("fn rx4_prompt_smoke"));
        assert!(lib.contains("fn rx4_agent_new"));
    }

    #[test]
    fn sha2_enrichment_exports_hash256() {
        let lib = sha2_lib_rs("0.10.9");
        assert!(lib.contains("fn sha2_abi_version"));
        assert!(lib.contains("fn sha2_hash_256"));
        assert!(lib.contains("Sha256"));
        let hdr = sha2_header();
        assert!(hdr.contains("sha2_hash_256"));
    }

    #[test]
    fn detects_cdylib_crate_type() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["cdylib", "rlib"]
"#,
        )
        .unwrap();
        assert!(crate_has_cdylib(&manifest));

        std::fs::write(
            &manifest,
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["rlib"]
"#,
        )
        .unwrap();
        assert!(!crate_has_cdylib(&manifest));
    }
}
