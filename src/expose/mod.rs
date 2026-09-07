pub mod c_host;
pub mod csharp_host;
pub mod d_host;
pub mod native;
pub mod nim_host;
pub mod rust_host;
pub mod shim;
pub mod stamp;
pub mod zig_host;

use crate::detect::Language;
use crate::manifest::{Dependency, Manifest};
use crate::resolve::ResolvedPackage;
use crate::util::AppCtx;
use anyhow::Result;
use std::path::PathBuf;

pub struct ExposeResult {
    pub out: PathBuf,
    pub consumer: Language,
}

/// Re-sync all expose artifacts for the manifest.
pub fn resync_all(ctx: &AppCtx) -> Result<Vec<ExposeResult>> {
    if !ctx.manifest.expose.enabled {
        return Ok(vec![]);
    }
    let mut outs = Vec::new();
    for (name, dep) in &ctx.manifest.dependencies {
        if !dep.expose {
            continue;
        }
        if let Some(r) = expose_one(ctx, name, dep, None)? {
            outs.push(r);
        }
    }
    write_mod_rs(ctx)?;
    stamp::write_stamp(ctx)?;
    Ok(outs)
}

pub fn expose_resolved(
    ctx: &AppCtx,
    resolved: &ResolvedPackage,
    dep: &Dependency,
) -> Result<Option<ExposeResult>> {
    if !ctx.manifest.expose.enabled || !dep.expose {
        return Ok(None);
    }
    let r = expose_one(ctx, &resolved.name, dep, Some(resolved))?;
    write_mod_rs(ctx)?;
    stamp::write_stamp(ctx)?;
    Ok(r)
}

fn expose_one(
    ctx: &AppCtx,
    name: &str,
    dep: &Dependency,
    resolved: Option<&ResolvedPackage>,
) -> Result<Option<ExposeResult>> {
    let host = ctx.host.language;
    let consumer = dep
        .expose_opts
        .as_ref()
        .and_then(|o| o.consumer.as_deref())
        .map(Language::parse)
        .transpose()?
        .unwrap_or(host);

    let out_rel = dep
        .expose_opts
        .as_ref()
        .and_then(|o| o.out.clone())
        .unwrap_or_else(|| default_out(&ctx.manifest, name, consumer));

    let out_path = ctx.root.join(&out_rel);
    crate::util::ensure_parent(&out_path)?;

    match (host, dep.ecosystem.as_str()) {
        (Language::Rust, "cargo") => {
            rust_host::write_rust_reexport(&out_path, name, dep)?;
            crate::util::edit::ensure_rust_mod_decl(&ctx.root)?;
        }
        (Language::Zig, "cargo") => {
            // Real C ABI: generate shim cdylib, build it, emit Zig imports.
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = dep
                .expose_opts
                .as_ref()
                .and_then(|o| o.native.clone())
                .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
            zig_host::write_zig_bindings(
                &out_path,
                name,
                dep,
                Some(&built.artifacts.header),
                &built.artifacts.lib_name,
                &native_rel,
            )?;
            // Also copy header next to bindings for @cImport consumers.
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::copy(
                    &built.artifacts.header,
                    parent.join(built.artifacts.header.file_name().unwrap()),
                );
            }
            let build_zig = ctx.root.join("build.zig");
            if build_zig.exists() {
                crate::util::edit::patch_build_zig_link(
                    &build_zig,
                    name,
                    &native_rel,
                    &built.artifacts.lib_name,
                )?;
            }
        }
        (Language::Nim, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = dep
                .expose_opts
                .as_ref()
                .and_then(|o| o.native.clone())
                .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
            nim_host::write_nim_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
            )?;
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::copy(
                    &built.artifacts.header,
                    parent.join(built.artifacts.header.file_name().unwrap()),
                );
            }
        }
        (Language::C | Language::Cpp, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = dep
                .expose_opts
                .as_ref()
                .and_then(|o| o.native.clone())
                .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
            c_host::write_c_bindings(
                &out_path,
                name,
                dep,
                Some(&built.artifacts.header),
                &built.artifacts.lib_name,
                &native_rel,
            )?;
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::copy(
                    &built.artifacts.header,
                    parent.join(built.artifacts.header.file_name().unwrap()),
                );
            }
        }
        (Language::CSharp, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = dep
                .expose_opts
                .as_ref()
                .and_then(|o| o.native.clone())
                .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
            csharp_host::write_csharp_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
            )?;
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::copy(
                    &built.artifacts.header,
                    parent.join(built.artifacts.header.file_name().unwrap()),
                );
            }
        }
        (Language::D, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = dep
                .expose_opts
                .as_ref()
                .and_then(|o| o.native.clone())
                .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
            d_host::write_d_bindings(&out_path, name, dep, &built.artifacts.lib_name, &native_rel)?;
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::copy(
                    &built.artifacts.header,
                    parent.join(built.artifacts.header.file_name().unwrap()),
                );
            }
        }
        (Language::Rust, eco) if eco != "cargo" => {
            rust_host::write_equilibrium_load_stub(&out_path, name, dep, eco)?;
            crate::util::edit::ensure_rust_mod_decl(&ctx.root)?;
        }
        (Language::Nim, eco) if eco != "cargo" => {
            nim_host::write_nim_path_git_stub(&out_path, name, dep)?;
        }
        (Language::C | Language::Cpp, eco) if eco != "cargo" => {
            c_host::write_c_path_git_stub(&out_path, name, dep)?;
        }
        _ => {
            rust_host::write_generic_stub(&out_path, name, dep, consumer)?;
        }
    }

    Ok(Some(ExposeResult {
        out: out_path,
        consumer,
    }))
}

fn default_out(manifest: &Manifest, name: &str, consumer: Language) -> String {
    let dir = &manifest.expose.dir;
    let safe = name.replace('-', "_");
    match consumer {
        Language::Rust => format!("{dir}/{safe}.rs"),
        Language::Zig => format!("{dir}/{safe}_bindings.zig"),
        Language::Nim => format!("{dir}/{safe}.nim"),
        Language::C | Language::Cpp => format!("{dir}/{safe}.h"),
        Language::CSharp => format!("{dir}/{safe}.cs"),
        Language::V => format!("{dir}/{safe}.v"),
        Language::D => format!("{dir}/{safe}.d"),
        Language::Odin => format!("{dir}/{safe}.odin"),
        Language::Hare => format!("{dir}/{safe}.ha"),
    }
}

fn write_mod_rs(ctx: &AppCtx) -> Result<()> {
    if ctx.host.language != Language::Rust {
        return Ok(());
    }
    let dir = ctx.root.join(&ctx.manifest.expose.dir);
    if !dir.is_dir() {
        return Ok(());
    }
    let mut mods = Vec::new();
    for ent in std::fs::read_dir(&dir)? {
        let ent = ent?;
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && stem != "mod"
        {
            mods.push(stem.to_string());
        }
    }
    mods.sort();
    let mut body = String::from("// @generated by rig — do not edit\n");
    for m in &mods {
        body.push_str(&format!("pub mod {m};\n"));
        body.push_str(&format!("pub use {m}::*;\n"));
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("mod.rs"), body)?;
    Ok(())
}

pub fn remove_expose_artifacts(ctx: &AppCtx, name: &str) -> Result<()> {
    let safe = name.replace('-', "_");
    let dir = ctx.root.join(&ctx.manifest.expose.dir);
    for ext in ["rs", "zig", "nim", "h", "cs", "v", "d", "odin", "ha"] {
        let p = dir.join(format!("{safe}.{ext}"));
        if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
        let p2 = dir.join(format!("{safe}_bindings.{ext}"));
        if p2.exists() {
            let _ = std::fs::remove_file(&p2);
        }
    }
    let shim = shim::shim_dir(ctx, name);
    if shim.exists() {
        let _ = std::fs::remove_dir_all(&shim);
    }
    let native = shim::native_out_dir(ctx, name);
    if native.exists() {
        let _ = std::fs::remove_dir_all(&native);
    }
    write_mod_rs(ctx)?;
    stamp::write_stamp(ctx)?;
    Ok(())
}
