pub mod api_scan;
pub mod c_header_scan;
pub mod c_host;
pub mod csharp_host;
pub mod d_host;
pub mod hare_host;
pub mod native;
pub mod nim_host;
pub mod odin_host;
pub mod path_native;
pub mod rust_host;
pub mod shim;
pub mod stamp;
pub mod surface;
pub mod v_host;
pub mod zig_export_scan;
pub mod zig_host;

use crate::detect::Language;
use crate::manifest::{Dependency, Manifest};
use crate::resolve::ResolvedPackage;
use crate::util::AppCtx;
use anyhow::{Result, bail};
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
                &built.artifacts.exports,
            )?;
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
            let native_rel = native_rel_for(ctx, name, dep);
            nim_host::write_nim_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
                &built.artifacts.exports,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        (Language::C | Language::Cpp, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = native_rel_for(ctx, name, dep);
            c_host::write_c_bindings(
                &out_path,
                name,
                dep,
                Some(&built.artifacts.header),
                &built.artifacts.lib_name,
                &native_rel,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        (Language::CSharp, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = native_rel_for(ctx, name, dep);
            csharp_host::write_csharp_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
                &built.artifacts.exports,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        (Language::D, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = native_rel_for(ctx, name, dep);
            d_host::write_d_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
                &built.artifacts.exports,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        (Language::V, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = native_rel_for(ctx, name, dep);
            v_host::write_v_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
                &built.artifacts.exports,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        (Language::Odin, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = native_rel_for(ctx, name, dep);
            odin_host::write_odin_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
                &built.artifacts.exports,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        (Language::Hare, "cargo") => {
            let built = native::build_cargo_cdylib(ctx, name, resolved, dep)?;
            let native_rel = native_rel_for(ctx, name, dep);
            hare_host::write_hare_bindings(
                &out_path,
                name,
                dep,
                &built.artifacts.lib_name,
                &native_rel,
                &built.artifacts.exports,
            )?;
            copy_header_beside(&out_path, &built.artifacts.header);
        }
        // Path/git non-cargo: real build+link when feasible (c/cpp/zig/nim/v/odin/hare).
        (Language::C | Language::Cpp, eco)
            if matches!(eco, "c" | "cpp" | "zig" | "nim" | "v" | "odin" | "hare") =>
        {
            match path_native::build_path_git_lib(ctx, name, dep, resolved) {
                Ok(built) => {
                    let mut include_names = Vec::new();
                    if let Some(parent) = out_path.parent() {
                        let out_name = out_path.file_name().and_then(|s| s.to_str());
                        for h in &built.headers {
                            let Some(fname) = h.file_name().and_then(|s| s.to_str()) else {
                                continue;
                            };
                            let dest_name = if Some(fname) == out_name {
                                let stem = PathBuf::from(fname)
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| fname.to_string());
                                format!("{stem}_api.h")
                            } else {
                                fname.to_string()
                            };
                            let _ = std::fs::copy(h, parent.join(&dest_name));
                            include_names.push(dest_name);
                        }
                    }
                    path_native::write_c_path_native_header(
                        &out_path,
                        name,
                        dep,
                        &built,
                        &include_names,
                    )?;
                }
                Err(err) => {
                    bail!(
                        "path/git expose build failed for `{name}` ({eco}): {err}\n\
                         Feasible auto-build: flat sources, Makefile ($OUT), CMakeLists.txt, \
                         meson.build, or a nim/v/odin/hare/zig tree with the toolchain on PATH."
                    );
                }
            }
        }
        (Language::Zig, eco)
            if matches!(eco, "c" | "cpp" | "zig" | "nim" | "v" | "odin" | "hare") =>
        {
            match path_native::build_path_git_lib(ctx, name, dep, resolved) {
                Ok(built) => {
                    path_native::write_zig_path_native_bindings(&out_path, name, dep, &built)?;
                    let build_zig = ctx.root.join("build.zig");
                    if build_zig.exists() {
                        crate::util::edit::patch_build_zig_link(
                            &build_zig,
                            name,
                            &built.native_rel,
                            &built.lib_name,
                        )?;
                    }
                }
                Err(err) => {
                    bail!(
                        "path/git expose build failed for `{name}` ({eco}): {err}\n\
                         Use path:… with compilable sources, or git+… that clones cleanly."
                    );
                }
            }
        }
        (Language::Nim, eco)
            if matches!(eco, "c" | "cpp" | "zig" | "nim" | "v" | "odin" | "hare") =>
        {
            match path_native::build_path_git_lib(ctx, name, dep, resolved) {
                Ok(built) => {
                    nim_host::write_nim_path_native(&out_path, name, dep, &built)?;
                }
                Err(err) => {
                    bail!(
                        "path/git expose build failed for `{name}` ({eco}): {err}\n\
                         Install the matching toolchain or simplify the vendor tree."
                    );
                }
            }
        }
        (Language::V, eco)
            if matches!(eco, "c" | "cpp" | "zig" | "nim" | "v" | "odin" | "hare") =>
        {
            match path_native::build_path_git_lib(ctx, name, dep, resolved) {
                Ok(built) => {
                    v_host::write_v_path_native(&out_path, name, dep, &built)?;
                }
                Err(err) => {
                    bail!("path/git expose build failed for `{name}` ({eco}): {err}");
                }
            }
        }
        (Language::Odin, eco)
            if matches!(eco, "c" | "cpp" | "zig" | "nim" | "v" | "odin" | "hare") =>
        {
            match path_native::build_path_git_lib(ctx, name, dep, resolved) {
                Ok(built) => {
                    odin_host::write_odin_path_native(&out_path, name, dep, &built)?;
                }
                Err(err) => {
                    bail!("path/git expose build failed for `{name}` ({eco}): {err}");
                }
            }
        }
        (Language::Hare, eco)
            if matches!(eco, "c" | "cpp" | "zig" | "nim" | "v" | "odin" | "hare") =>
        {
            match path_native::build_path_git_lib(ctx, name, dep, resolved) {
                Ok(built) => {
                    hare_host::write_hare_path_native(&out_path, name, dep, &built)?;
                }
                Err(err) => {
                    bail!("path/git expose build failed for `{name}` ({eco}): {err}");
                }
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

fn native_rel_for(ctx: &AppCtx, name: &str, dep: &Dependency) -> String {
    dep.expose_opts
        .as_ref()
        .and_then(|o| o.native.clone())
        .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name))
}

fn copy_header_beside(out_path: &std::path::Path, header: &std::path::Path) {
    if let Some(parent) = out_path.parent()
        && let Some(fname) = header.file_name()
    {
        let _ = std::fs::copy(header, parent.join(fname));
    }
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
