use crate::detect::Language;
use crate::manifest::Dependency;
use anyhow::{Context, Result};
use std::path::Path;

pub fn write_rust_reexport(out: &Path, name: &str, dep: &Dependency) -> Result<()> {
    let crate_ident = name.replace('-', "_");
    let ver = dep.version.as_deref().unwrap_or("*");
    let body = format!(
        r#"//! Auto-exposed by rig — Rust host re-export of `{name}` ({ver}).
//! Native in-process path: link as a normal Cargo dependency, then `use` this module.
//! For cross-lang hosts, equilibrium-ffi generates consumer imports from a C ABI surface.
#![allow(unused_imports)]

pub use {crate_ident}::*;

/// Marker that rig wired `{name}` into this project.
pub fn rig_native_{crate_ident}() -> &'static str {{
    "{name}"
}}

/// Documents the equilibrium-ffi load path when a foreign/cdylib surface is available.
pub fn rig_equilibrium_hint_{crate_ident}() -> &'static str {{
    "equilibrium_ffi::load / generate_imports — see https://github.com/tschk/equilibrium"
}}
"#
    );
    std::fs::write(out, body).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

pub fn write_equilibrium_load_stub(
    out: &Path,
    name: &str,
    dep: &Dependency,
    eco: &str,
) -> Result<()> {
    let crate_ident = name.replace('-', "_");
    let load_path = dep
        .path
        .clone()
        .or_else(|| dep.git.clone())
        .unwrap_or_else(|| format!("vendor/{name}"));
    let body = format!(
        r#"//! Auto-exposed by rig — Rust host ← {eco} package `{name}` via equilibrium-ffi.
#![allow(dead_code)]

/// Path hint for `equilibrium_ffi::load(...)`.
pub const RIG_LOAD_PATH: &str = "{load_path}";

pub fn rig_load_{crate_ident}_hint() -> &'static str {{
    RIG_LOAD_PATH
}}
"#
    );
    std::fs::write(out, body).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

pub fn write_generic_stub(
    out: &Path,
    name: &str,
    dep: &Dependency,
    consumer: Language,
) -> Result<()> {
    let body = format!(
        "//! rig expose stub for `{name}` (ecosystem={}, consumer={consumer})\n//! Wire via equilibrium-ffi generate_imports.\n",
        dep.ecosystem
    );
    std::fs::write(out, body)?;
    Ok(())
}
