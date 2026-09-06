//! Thin cdylib façade generation for cargo packages that lack a C ABI.
//!
//! crates.io `rx4` is `rlib`-only and `#![forbid(unsafe_code)]`, so Zig hosts
//! cannot link it directly. rig materializes `.rig/shims/<pkg>/` — a small
//! `cdylib` that re-exports a narrow `extern "C"` surface (e.g. `rx4_version`).

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

/// Ensure a façade crate exists for `name`. Returns paths used by native build.
pub fn ensure_shim(
    ctx: &AppCtx,
    name: &str,
    resolved: Option<&ResolvedPackage>,
    dep: &Dependency,
) -> Result<ShimArtifacts> {
    let dir = shim_dir(ctx, name);
    std::fs::create_dir_all(dir.join("src")).with_context(|| format!("mkdir {}", dir.display()))?;

    let lib_name = format!("{}_ffi", name.replace('-', "_"));
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
        _ => generic_lib_rs(name, version),
    };
    std::fs::write(dir.join("src/lib.rs"), lib_rs)?;

    let header = match name {
        "rx4" | "rotary" => rx4_header(),
        _ => generic_header(name, &lib_name),
    };
    let header_path = dir.join(format!("{lib_name}.h"));
    std::fs::write(&header_path, header)?;

    let out_dir = native_out_dir(ctx, name);
    std::fs::create_dir_all(&out_dir)?;

    Ok(ShimArtifacts {
        shim_dir: dir,
        header: header_path,
        lib_name,
        out_dir,
    })
}

fn cargo_dep_line(
    name: &str,
    resolved: Option<&ResolvedPackage>,
    dep: &Dependency,
) -> Result<String> {
    let crate_name = if name == "rotary" { "rx4" } else { name };

    // Prefer explicit path / git from resolution or manifest.
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

    // Local developer convenience: sibling rotary checkout.
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

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::OnceLock;

/// ABI revision for this façade (bump when symbols/semantics change).
#[no_mangle]
pub extern "C" fn rx4_abi_version() -> u32 {
    1
}

/// Null-terminated `rx4::VERSION` from the linked rx4 crate.
#[no_mangle]
pub extern "C" fn rx4_version() -> *const c_char {
    static V: OnceLock<CString> = OnceLock::new();
    V.get_or_init(|| CString::new(rx4::VERSION).expect("rx4::VERSION has interior NUL"))
        .as_ptr()
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

/** Façade ABI revision (not the rx4 crate semver). */
uint32_t rx4_abi_version(void);

/** Null-terminated rx4 crate version string (static storage). */
const char *rx4_version(void);

#ifdef __cplusplus
}
#endif

#endif /* RIG_RX4_FFI_H */
"#
    .into()
}

fn generic_lib_rs(name: &str, version: &str) -> String {
    let safe = name.replace('-', "_");
    format!(
        r#"//! rig-generated placeholder C ABI for `{name}` (no known façade surface).
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn {safe}_rig_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

#[no_mangle]
pub extern "C" fn {safe}_rig_abi_version() -> u32 {{
    1
}}
"#
    )
}

fn generic_header(name: &str, lib_name: &str) -> String {
    let safe = name.replace('-', "_");
    format!(
        r#"/* Auto-generated by rig — placeholder C ABI for {name} ({lib_name}) */
#ifndef RIG_{guard}_H
#define RIG_{guard}_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {{
#endif
uint32_t {safe}_rig_abi_version(void);
const char *{safe}_rig_version(void);
#ifdef __cplusplus
}}
#endif
#endif
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
        // Windows MSVC
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
    // Also copy import lib on windows if present — skipped for MVP.
    let hdr_dest = artifacts
        .out_dir
        .join(artifacts.header.file_name().unwrap());
    std::fs::copy(&artifacts.header, &hdr_dest)?;
    // Convenience copy as <pkg>.h
    if let Some(pkg) = artifacts.shim_dir.file_name() {
        let _ = std::fs::copy(
            &artifacts.header,
            artifacts
                .out_dir
                .join(format!("{}.h", pkg.to_string_lossy())),
        );
    }
    Ok(dest)
}
