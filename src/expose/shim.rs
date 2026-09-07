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
//! Auto-wrap: when sources are available, rig scans for simple `pub fn` /
//! existing `extern "C"` / `extern "C-unwind"` surfaces and exports them from
//! the façade (see `api_scan`). Niche-safe types (`Option<*T>`, `NonNull`,
//! `NonZero*`, `*const ()`) are adapted at the façade boundary. Honest limits
//! remain: generics, traits/`impl` methods, async, tuples/refs/`str`/`String`/
//! `Vec`, `Option<scalar>`, and other non-FFI-safe types are skipped.
//! Optional `cbindgen` runs only when the crate ships `cbindgen.toml` and the
//! `cbindgen` binary is on `PATH`.

use crate::expose::api_scan::{
    ExportFn, ExportKind, FfiType, Param, ScanReport, locate_or_fetch_sources, scan_crate_sources,
    try_cbindgen,
};
use crate::expose::surface;
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
    /// Full callable surface (markers + enrichments + auto-wraps).
    pub exports: Vec<ExportFn>,
    /// Scanner summary (skipped counts); empty for enrichments/passthrough.
    pub scan: ScanReport,
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
            let (header_path, exports) = write_passthrough_header(&out_dir, name, &lib_name)?;
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
                exports,
                scan: ScanReport::default(),
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

    let path_hint = resolve_dep_path(name, resolved, dep);
    let cache = ctx.root.join(&ctx.manifest.expose.cache).join("crates");

    let (lib_rs, header, exports, scan) = match name {
        "rx4" | "rotary" => {
            let exports = rx4_exports();
            (
                rx4_lib_rs(),
                header_from_exports(name, &lib_name, &exports, None),
                exports,
                ScanReport::default(),
            )
        }
        "sha2" => {
            let exports = sha2_exports();
            (
                sha2_lib_rs(version),
                header_from_exports(name, &lib_name, &exports, None),
                exports,
                ScanReport::default(),
            )
        }
        "crc32fast" => {
            let exports = crc32fast_exports();
            (
                crc32fast_lib_rs(version),
                header_from_exports(name, &lib_name, &exports, None),
                exports,
                ScanReport::default(),
            )
        }
        "md-5" | "md5" => {
            let exports = md5_exports();
            (
                md5_lib_rs(version),
                header_from_exports("md5", &lib_name, &exports, None),
                exports,
                ScanReport::default(),
            )
        }
        "hex" => {
            let exports = hex_exports();
            (
                hex_lib_rs(version),
                header_from_exports("hex", &lib_name, &exports, None),
                exports,
                ScanReport::default(),
            )
        }
        _ => build_generic_surface(name, version, path_hint.as_deref(), &cache, &lib_name, &dir)?,
    };

    std::fs::write(dir.join("src/lib.rs"), &lib_rs)?;
    let header_path = dir.join(format!("{lib_name}.h"));
    std::fs::write(&header_path, &header)?;
    let _ = write_surface_json(
        &dir.join("surface.json"),
        name,
        version,
        &lib_name,
        &exports,
        &scan,
    );

    Ok(ShimArtifacts {
        shim_dir: dir,
        header: header_path,
        lib_name,
        out_dir,
        passthrough: false,
        exports,
        scan,
    })
}

fn build_generic_surface(
    name: &str,
    version: &str,
    path_hint: Option<&Path>,
    cache: &Path,
    lib_name: &str,
    dir: &Path,
) -> Result<(String, String, Vec<ExportFn>, ScanReport)> {
    let mut scan = ScanReport::default();
    let mut wraps: Vec<ExportFn> = Vec::new();
    if let Some(src) = locate_or_fetch_sources(name, version, path_hint, cache) {
        scan = scan_crate_sources(&src, name);
        const MAX_AUTO: usize = 256;
        wraps = scan
            .exports
            .iter()
            .filter(|e| matches!(e.kind, ExportKind::AutoWrap | ExportKind::UpstreamExternC))
            .take(MAX_AUTO)
            .cloned()
            .collect();
        let cbind_out = dir.join("cbindgen_prov.h");
        if try_cbindgen(&src, &cbind_out) {
            let _ = std::fs::write(
                dir.join("CBINDGEN"),
                format!(
                    "cbindgen.toml detected; provenance at {}\n",
                    cbind_out.display()
                ),
            );
        }
    }
    let mut exports = marker_exports(name, version);
    exports.extend(wraps);
    let note = scan_note(&scan);
    let lib_rs = generic_lib_rs_with_wraps(name, version, &exports, &note);
    let header = header_from_exports(name, lib_name, &exports, Some(&note));
    Ok((lib_rs, header, exports, scan))
}

fn scan_note(scan: &ScanReport) -> String {
    let callable = scan
        .exports
        .iter()
        .filter(|e| matches!(e.kind, ExportKind::AutoWrap | ExportKind::UpstreamExternC))
        .count();
    format!(
        "auto-wrap: {callable} callable; skipped generics={} async={} unfriendly={} impl_methods={}",
        scan.skipped_generics,
        scan.skipped_async,
        scan.skipped_unfriendly,
        scan.skipped_impl_methods
    )
}

fn write_surface_json(
    path: &Path,
    name: &str,
    version: &str,
    lib_name: &str,
    exports: &[ExportFn],
    scan: &ScanReport,
) -> Result<()> {
    let names: Vec<&str> = exports.iter().map(|e| e.export_name.as_str()).collect();
    let body = serde_json::json!({
        "package": name,
        "version": version,
        "lib_name": lib_name,
        "exports": names,
        "skipped": {
            "generics": scan.skipped_generics,
            "async": scan.skipped_async,
            "unfriendly": scan.skipped_unfriendly,
            "impl_methods": scan.skipped_impl_methods,
        },
        "source_root": scan.source_root.as_ref().map(|p| p.display().to_string()),
    });
    std::fs::write(path, serde_json::to_string_pretty(&body)?)?;
    Ok(())
}

fn marker_exports(name: &str, _version: &str) -> Vec<ExportFn> {
    let safe = crate_ident(name);
    vec![
        ExportFn {
            export_name: format!("{safe}_abi_version"),
            rust_callee: None,
            params: vec![],
            ret: FfiType::U32,
            ret_adapt: Default::default(),
            kind: ExportKind::Marker,
            is_unsafe: false,
        },
        ExportFn {
            export_name: format!("{safe}_version"),
            rust_callee: None,
            params: vec![],
            ret: FfiType::ConstCChar,
            ret_adapt: Default::default(),
            kind: ExportKind::Marker,
            is_unsafe: false,
        },
        ExportFn {
            export_name: format!("{safe}_name"),
            rust_callee: None,
            params: vec![],
            ret: FfiType::ConstCChar,
            ret_adapt: Default::default(),
            kind: ExportKind::Marker,
            is_unsafe: false,
        },
    ]
}

fn sha2_exports() -> Vec<ExportFn> {
    let mut v = marker_exports("sha2", "");
    let hash_params = vec![
        Param {
            name: "data".into(),
            ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
            adapt: Default::default(),
        },
        Param {
            name: "len".into(),
            ty: FfiType::Usize,
            adapt: Default::default(),
        },
        Param {
            name: "out".into(),
            ty: FfiType::MutPtr(Box::new(FfiType::U8)),
            adapt: Default::default(),
        },
    ];
    v.push(ExportFn {
        export_name: "sha2_hash_256".into(),
        rust_callee: None,
        params: hash_params.clone(),
        ret: FfiType::I32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v.push(ExportFn {
        export_name: "sha2_hash_512".into(),
        rust_callee: None,
        params: hash_params,
        ret: FfiType::I32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v
}

fn crc32fast_exports() -> Vec<ExportFn> {
    let mut v = marker_exports("crc32fast", "");
    v.push(ExportFn {
        export_name: "crc32fast_hash".into(),
        rust_callee: None,
        params: vec![
            Param {
                name: "data".into(),
                ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
            Param {
                name: "len".into(),
                ty: FfiType::Usize,
                adapt: Default::default(),
            },
        ],
        ret: FfiType::U32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v
}

fn md5_exports() -> Vec<ExportFn> {
    let mut v = marker_exports("md5", "");
    v.push(ExportFn {
        export_name: "md5_hash".into(),
        rust_callee: None,
        params: vec![
            Param {
                name: "data".into(),
                ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
            Param {
                name: "len".into(),
                ty: FfiType::Usize,
                adapt: Default::default(),
            },
            Param {
                name: "out".into(),
                ty: FfiType::MutPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
        ],
        ret: FfiType::I32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v
}

fn rx4_exports() -> Vec<ExportFn> {
    let mut v = marker_exports("rx4", "");
    v.push(ExportFn {
        export_name: "rx4_agent_new".into(),
        rust_callee: None,
        params: vec![],
        ret: FfiType::MutVoid,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v.push(ExportFn {
        export_name: "rx4_agent_free".into(),
        rust_callee: None,
        params: vec![Param {
            name: "agent".into(),
            ty: FfiType::MutVoid,
            adapt: Default::default(),
        }],
        ret: FfiType::Void,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v.push(ExportFn {
        export_name: "rx4_prompt_smoke".into(),
        rust_callee: None,
        params: vec![
            Param {
                name: "agent".into(),
                ty: FfiType::MutVoid,
                adapt: Default::default(),
            },
            Param {
                name: "prompt".into(),
                ty: FfiType::ConstCChar,
                adapt: Default::default(),
            },
        ],
        ret: FfiType::I32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v
}

fn header_from_exports(
    name: &str,
    lib_name: &str,
    exports: &[ExportFn],
    note: Option<&str>,
) -> String {
    let safe = crate_ident(name);
    let guard = format!("RIG_{}_FFI_H", safe.to_uppercase());
    let mut includes = String::from("#include <stdint.h>\n");
    if surface::needs_stddef(exports) {
        includes.push_str("#include <stddef.h>\n");
    }
    if surface::needs_stdbool(exports) {
        includes.push_str("#include <stdbool.h>\n");
    }
    let note_line = note.map(|n| format!(" * {n}\n")).unwrap_or_default();
    format!(
        "/* Auto-generated by rig — C ABI façade for {name} ({lib_name})\n\
{note_line} */\n\
#ifndef {guard}\n\
#define {guard}\n\
{includes}\
#ifdef __cplusplus\n\
extern \"C\" {{\n\
#endif\n\
{}\
#ifdef __cplusplus\n\
}}\n\
#endif\n\
#endif /* {guard} */\n",
        surface::emit_c_decls(exports)
    )
}

fn write_passthrough_header(
    out_dir: &Path,
    name: &str,
    lib_name: &str,
) -> Result<(PathBuf, Vec<ExportFn>)> {
    let exports = marker_exports(name, "");
    let header = header_from_exports(name, lib_name, &exports, Some("passthrough markers only"));
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
    Ok((path, exports))
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

#[allow(dead_code)]
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
        r#"//! rig-generated C ABI façade for `sha2` (markers + SHA-256/512 helpers).
//! Domain helpers are known enrichments — not a full digest API.

use sha2::{{Digest, Sha256, Sha512}};
use std::os::raw::c_char;
use std::slice;

#[no_mangle]
pub extern "C" fn sha2_abi_version() -> u32 {{
    3
}}

#[no_mangle]
pub extern "C" fn sha2_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

#[no_mangle]
pub extern "C" fn sha2_name() -> *const c_char {{
    b"sha2\0".as_ptr() as *const c_char
}}

/// Hash `len` bytes at `data` into a 32-byte `out` buffer (SHA-256).
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

/// Hash `len` bytes at `data` into a 64-byte `out` buffer (SHA-512).
///
/// Returns `0` on success, `-1` if `data` is null with nonzero len, `-2` if `out` is null.
#[no_mangle]
pub extern "C" fn sha2_hash_512(data: *const u8, len: usize, out: *mut u8) -> i32 {{
    if out.is_null() {{
        return -2;
    }}
    if len > 0 && data.is_null() {{
        return -1;
    }}
    let input = if len == 0 {{
        &[][..]
    }} else {{
        unsafe {{ slice::from_raw_parts(data, len) }}
    }};
    let digest = Sha512::digest(input);
    unsafe {{
        slice::from_raw_parts_mut(out, 64).copy_from_slice(&digest);
    }}
    0
}}
"#
    )
}

#[allow(dead_code)]
fn sha2_header() -> String {
    r#"/* Auto-generated by rig — C ABI façade for sha2 (ABI 3) */
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
/** SHA-512: write 64 bytes to out. 0 ok; -1 null data; -2 null out. */
int32_t sha2_hash_512(const uint8_t *data, size_t len, uint8_t *out);
#ifdef __cplusplus
}
#endif
#endif /* RIG_SHA2_FFI_H */
"#
    .into()
}

fn crc32fast_lib_rs(version: &str) -> String {
    format!(
        r#"//! rig-generated C ABI façade for `crc32fast` (markers + hash helper).

use std::os::raw::c_char;
use std::slice;

#[no_mangle]
pub extern "C" fn crc32fast_abi_version() -> u32 {{
    2
}}

#[no_mangle]
pub extern "C" fn crc32fast_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

#[no_mangle]
pub extern "C" fn crc32fast_name() -> *const c_char {{
    b"crc32fast\0".as_ptr() as *const c_char
}}

/// CRC-32 (Castagnoli/IEEE via crc32fast) over `len` bytes at `data`.
///
/// Returns the checksum. Null `data` with nonzero `len` yields `0` (treat as empty miss —
/// callers should validate pointers).
#[no_mangle]
pub extern "C" fn crc32fast_hash(data: *const u8, len: usize) -> u32 {{
    if len == 0 || data.is_null() {{
        return crc32fast::hash(&[]);
    }}
    let input = unsafe {{ slice::from_raw_parts(data, len) }};
    crc32fast::hash(input)
}}
"#
    )
}

fn md5_lib_rs(version: &str) -> String {
    format!(
        r#"//! rig-generated C ABI façade for `md-5` (markers + MD5 helper).
//! Known enrichment — MD5 is not for new security-sensitive designs.

use md5::{{Digest, Md5}};
use std::os::raw::c_char;
use std::slice;

#[no_mangle]
pub extern "C" fn md5_abi_version() -> u32 {{
    2
}}

#[no_mangle]
pub extern "C" fn md5_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

#[no_mangle]
pub extern "C" fn md5_name() -> *const c_char {{
    b"md-5\0".as_ptr() as *const c_char
}}

/// MD5: write 16 bytes to `out`. 0 ok; -1 null data; -2 null out.
#[no_mangle]
pub extern "C" fn md5_hash(data: *const u8, len: usize, out: *mut u8) -> i32 {{
    if out.is_null() {{
        return -2;
    }}
    if len > 0 && data.is_null() {{
        return -1;
    }}
    let input = if len == 0 {{
        &[][..]
    }} else {{
        unsafe {{ slice::from_raw_parts(data, len) }}
    }};
    let digest = Md5::digest(input);
    unsafe {{
        slice::from_raw_parts_mut(out, 16).copy_from_slice(&digest);
    }}
    0
}}
"#
    )
}

fn hex_exports() -> Vec<ExportFn> {
    let mut v = marker_exports("hex", "");
    v.push(ExportFn {
        export_name: "hex_encode_len".into(),
        rust_callee: None,
        params: vec![Param {
            name: "len".into(),
            ty: FfiType::Usize,
            adapt: Default::default(),
        }],
        ret: FfiType::Usize,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v.push(ExportFn {
        export_name: "hex_encode".into(),
        rust_callee: None,
        params: vec![
            Param {
                name: "data".into(),
                ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
            Param {
                name: "len".into(),
                ty: FfiType::Usize,
                adapt: Default::default(),
            },
            Param {
                name: "out".into(),
                ty: FfiType::MutPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
            Param {
                name: "out_len".into(),
                ty: FfiType::Usize,
                adapt: Default::default(),
            },
        ],
        ret: FfiType::I32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v.push(ExportFn {
        export_name: "hex_decode".into(),
        rust_callee: None,
        params: vec![
            Param {
                name: "data".into(),
                ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
            Param {
                name: "len".into(),
                ty: FfiType::Usize,
                adapt: Default::default(),
            },
            Param {
                name: "out".into(),
                ty: FfiType::MutPtr(Box::new(FfiType::U8)),
                adapt: Default::default(),
            },
            Param {
                name: "out_len".into(),
                ty: FfiType::Usize,
                adapt: Default::default(),
            },
        ],
        ret: FfiType::I32,
        ret_adapt: Default::default(),
        kind: ExportKind::Enrichment,
        is_unsafe: false,
    });
    v
}

fn hex_lib_rs(version: &str) -> String {
    format!(
        r#"//! rig-generated C ABI façade for `hex` (markers + encode/decode helpers).

use std::os::raw::c_char;
use std::slice;

#[no_mangle]
pub extern "C" fn hex_abi_version() -> u32 {{
    2
}}

#[no_mangle]
pub extern "C" fn hex_version() -> *const c_char {{
    concat!("{version}", "\0").as_ptr() as *const c_char
}}

#[no_mangle]
pub extern "C" fn hex_name() -> *const c_char {{
    b"hex\0".as_ptr() as *const c_char
}}

/// Bytes needed to hex-encode `len` input bytes (no NUL).
#[no_mangle]
pub extern "C" fn hex_encode_len(len: usize) -> usize {{
    len.saturating_mul(2)
}}

/// Hex-encode `len` bytes at `data` into `out` (must be >= 2*len).
/// Returns 0 ok; -1 null data; -2 null out; -3 out_len too small.
#[no_mangle]
pub extern "C" fn hex_encode(
    data: *const u8,
    len: usize,
    out: *mut u8,
    out_len: usize,
) -> i32 {{
    if out.is_null() {{
        return -2;
    }}
    if len > 0 && data.is_null() {{
        return -1;
    }}
    let need = len.saturating_mul(2);
    if out_len < need {{
        return -3;
    }}
    let input = if len == 0 {{
        &[][..]
    }} else {{
        unsafe {{ slice::from_raw_parts(data, len) }}
    }};
    let encoded = hex::encode(input);
    unsafe {{
        slice::from_raw_parts_mut(out, need).copy_from_slice(encoded.as_bytes());
    }}
    0
}}

/// Hex-decode `len` ASCII hex bytes at `data` into `out` (must be >= len/2).
/// Returns 0 ok; -1 null data; -2 null out; -3 out_len too small; -4 invalid hex.
#[no_mangle]
pub extern "C" fn hex_decode(
    data: *const u8,
    len: usize,
    out: *mut u8,
    out_len: usize,
) -> i32 {{
    if out.is_null() {{
        return -2;
    }}
    if len > 0 && data.is_null() {{
        return -1;
    }}
    let need = len / 2;
    if out_len < need {{
        return -3;
    }}
    let input = if len == 0 {{
        &[][..]
    }} else {{
        unsafe {{ slice::from_raw_parts(data, len) }}
    }};
    match hex::decode(input) {{
        Ok(bytes) => {{
            unsafe {{
                slice::from_raw_parts_mut(out, bytes.len()).copy_from_slice(&bytes);
            }}
            0
        }}
        Err(_) => -4,
    }}
}}
"#
    )
}

/// Generate generic façade `lib.rs` for an arbitrary cargo package (markers only).
pub fn generic_lib_rs(name: &str, version: &str) -> String {
    generic_lib_rs_with_wraps(
        name,
        version,
        &marker_exports(name, version),
        "markers only",
    )
}

fn generic_lib_rs_with_wraps(
    name: &str,
    version: &str,
    exports: &[ExportFn],
    note: &str,
) -> String {
    let safe = crate_ident(name);
    let abi = if exports
        .iter()
        .any(|e| matches!(e.kind, ExportKind::AutoWrap | ExportKind::UpstreamExternC))
    {
        2
    } else {
        1
    };
    let wraps = surface::emit_rust_wrappers(exports);
    format!(
        r#"//! rig-generated C ABI façade for `{name}`.
//! {note}
//! This façade crate may contain `unsafe` even when `{name}` forbids it.

use std::os::raw::c_char;

#[allow(dead_code, unused_imports)]
use {safe};

/// Façade ABI revision (2 when auto-wrap exports are present).
#[no_mangle]
pub extern "C" fn {safe}_abi_version() -> u32 {{
    {abi}
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
{wraps}"#
    )
}

/// Generate generic C header matching markers (+ optional auto-wrap decls).
pub fn generic_header(name: &str, lib_name: &str) -> String {
    header_from_exports(name, lib_name, &marker_exports(name, ""), None)
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
        assert!(lib.contains("use sha2"));
        let hdr = generic_header("sha2", "sha2_ffi");
        assert!(hdr.contains("sha2_abi_version"));
        assert!(hdr.contains("sha2_name"));
    }

    #[test]
    fn hyphenated_crate_idents() {
        let lib = generic_lib_rs("crypto-common", "0.1.0");
        assert!(lib.contains("fn crypto_common_abi_version"));
        assert!(lib.contains("use crypto_common"));
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
        assert!(lib.contains("fn sha2_hash_512"));
        assert!(lib.contains("Sha256"));
        assert!(lib.contains("Sha512"));
        assert!(lib.contains("3"), "ABI 3 for sha512 enrichment");
        let hdr = sha2_header();
        assert!(hdr.contains("sha2_hash_256"));
        assert!(hdr.contains("sha2_hash_512"));
    }

    #[test]
    fn crc32fast_enrichment_exports_hash() {
        let lib = crc32fast_lib_rs("1.4.2");
        assert!(lib.contains("fn crc32fast_abi_version"));
        assert!(lib.contains("fn crc32fast_hash"));
        assert!(lib.contains("crc32fast::hash"));
        let exports = crc32fast_exports();
        assert!(exports.iter().any(|e| e.export_name == "crc32fast_hash"));
    }

    #[test]
    fn md5_enrichment_exports_hash() {
        let lib = md5_lib_rs("0.10.6");
        assert!(lib.contains("fn md5_abi_version"));
        assert!(lib.contains("fn md5_hash"));
        assert!(lib.contains("Md5::digest"));
        let exports = md5_exports();
        assert!(exports.iter().any(|e| e.export_name == "md5_hash"));
    }

    #[test]
    fn hex_enrichment_exports_encode_decode() {
        let lib = hex_lib_rs("0.4.3");
        assert!(lib.contains("fn hex_abi_version"));
        assert!(lib.contains("fn hex_encode"));
        assert!(lib.contains("fn hex_decode"));
        assert!(lib.contains("hex::encode"));
        let exports = hex_exports();
        assert!(exports.iter().any(|e| e.export_name == "hex_encode"));
        assert!(exports.iter().any(|e| e.export_name == "hex_decode"));
    }

    #[test]
    fn auto_wrap_emits_prefixed_exports() {
        let mut exports = marker_exports("simple_api", "0.1.0");
        exports.push(ExportFn {
            export_name: "simple_api_add".into(),
            rust_callee: Some("simple_api::add".into()),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: FfiType::I32,
                    adapt: Default::default(),
                },
                Param {
                    name: "b".into(),
                    ty: FfiType::I32,
                    adapt: Default::default(),
                },
            ],
            ret: FfiType::I32,
            ret_adapt: Default::default(),
            kind: ExportKind::AutoWrap,
            is_unsafe: false,
        });
        let lib = generic_lib_rs_with_wraps("simple_api", "0.1.0", &exports, "test");
        assert!(lib.contains("fn simple_api_abi_version"));
        assert!(lib.contains("fn simple_api_add"));
        assert!(lib.contains("simple_api::add"));
        assert!(lib.contains("2"), "ABI should bump toward 2: {lib}");
        let hdr = header_from_exports("simple_api", "simple_api_ffi", &exports, None);
        assert!(hdr.contains("simple_api_add"));
        assert!(hdr.contains("int32_t"));
    }

    #[test]
    fn niche_adapt_emits_option_conversions() {
        use crate::expose::api_scan::TypeAdapt;
        use crate::expose::surface;
        let mut exports = marker_exports("niche_api", "0.1.0");
        exports.push(ExportFn {
            export_name: "niche_api_take_opt".into(),
            rust_callee: Some("niche_api::take_opt".into()),
            params: vec![Param {
                name: "p".into(),
                ty: FfiType::MutPtr(Box::new(FfiType::U8)),
                adapt: TypeAdapt::OptionPtr,
            }],
            ret: FfiType::MutPtr(Box::new(FfiType::U8)),
            ret_adapt: TypeAdapt::OptionPtr,
            kind: ExportKind::AutoWrap,
            is_unsafe: false,
        });
        let lib = generic_lib_rs_with_wraps("niche_api", "0.1.0", &exports, "test");
        assert!(lib.contains("fn niche_api_take_opt"), "{lib}");
        assert!(lib.contains("is_null()"), "{lib}");
        assert!(lib.contains("unwrap_or"), "{lib}");
        let _ = surface::emit_rust_wrappers(&exports);
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
