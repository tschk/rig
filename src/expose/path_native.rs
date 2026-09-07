//! Build path/git non-cargo deps (c / cpp / zig) into `target/rig/<pkg>/` when feasible.

use crate::manifest::Dependency;
use crate::resolve::ResolvedPackage;
use crate::util::AppCtx;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PathNativeBuild {
    pub lib_name: String,
    pub native_dir: PathBuf,
    pub native_rel: String,
    pub lib_path: PathBuf,
    pub headers: Vec<PathBuf>,
    pub source_root: PathBuf,
}

/// Resolve source tree for a path/git pin, build a shared library when feasible,
/// install into expose `build_dir`, and return link metadata.
pub fn build_path_git_lib(
    ctx: &AppCtx,
    name: &str,
    dep: &Dependency,
    resolved: Option<&ResolvedPackage>,
) -> Result<PathNativeBuild> {
    let eco = dep.ecosystem.as_str();
    if !matches!(eco, "c" | "cpp" | "zig") {
        bail!(
            "path/git native build only supports c/cpp/zig ecosystems (got `{eco}` for `{name}`)"
        );
    }

    let source_root = locate_or_fetch_path_git(ctx, name, dep, resolved)?;
    if !source_root.is_dir() {
        bail!(
            "path/git source for `{name}` is missing or not a directory: {}",
            source_root.display()
        );
    }

    let native_rel = dep
        .expose_opts
        .as_ref()
        .and_then(|o| o.native.clone())
        .unwrap_or_else(|| format!("{}/{}", ctx.manifest.expose.build_dir, name));
    let native_dir = ctx.root.join(&native_rel);
    std::fs::create_dir_all(&native_dir)
        .with_context(|| format!("mkdir {}", native_dir.display()))?;

    let safe = name.replace('-', "_");
    let lib_name = format!("{safe}_native");
    let dylib = shared_lib_path(&native_dir, &lib_name);

    match eco {
        "zig" => build_zig_shared(&source_root, name, &lib_name, &dylib)?,
        "cpp" => build_cc_family(&source_root, name, &lib_name, &dylib, true)?,
        _ => build_cc_family(&source_root, name, &lib_name, &dylib, false)?,
    }

    if !dylib.is_file() {
        bail!(
            "native build for `{name}` did not produce shared library at {}",
            dylib.display()
        );
    }

    let headers = collect_headers(&source_root);
    for h in &headers {
        if let Some(fname) = h.file_name() {
            let _ = std::fs::copy(h, native_dir.join(fname));
        }
    }

    Ok(PathNativeBuild {
        lib_name,
        native_dir,
        native_rel,
        lib_path: dylib,
        headers,
        source_root,
    })
}

fn locate_or_fetch_path_git(
    ctx: &AppCtx,
    name: &str,
    dep: &Dependency,
    resolved: Option<&ResolvedPackage>,
) -> Result<PathBuf> {
    if let Some(p) = dep
        .path
        .as_deref()
        .or_else(|| resolved.and_then(|r| r.path.as_deref()))
    {
        let path = PathBuf::from(p);
        let abs = if path.is_absolute() {
            path
        } else {
            ctx.root.join(path)
        };
        return Ok(abs);
    }

    let git = dep
        .git
        .as_deref()
        .or_else(|| resolved.and_then(|r| r.git.as_deref()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "`{name}` ({}) needs path:… or git+… for a non-cargo expose build",
                dep.ecosystem
            )
        })?;

    let cache = ctx
        .root
        .join(&ctx.manifest.expose.cache)
        .join("git")
        .join(name);
    if cache.join(".git").is_dir() {
        // Best-effort refresh; ignore failures (offline / dirty).
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&cache)
            .arg("pull")
            .arg("--ff-only")
            .status();
        return Ok(cache);
    }
    if cache.exists() {
        let _ = std::fs::remove_dir_all(&cache);
    }
    std::fs::create_dir_all(cache.parent().unwrap())?;
    let status = Command::new("git")
        .args(["clone", "--depth", "1", git])
        .arg(&cache)
        .status()
        .with_context(|| format!("spawn git clone for {git}"))?;
    if !status.success() {
        bail!(
            "git clone failed for `{name}` from {git} (status {status}).\n\
             Fix: ensure git is installed and the URL is reachable, or use path:… instead."
        );
    }
    Ok(cache)
}

fn shared_lib_path(dir: &Path, lib_name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dir.join(format!("{lib_name}.dll"))
    }
    #[cfg(target_os = "macos")]
    {
        dir.join(format!("lib{lib_name}.dylib"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dir.join(format!("lib{lib_name}.so"))
    }
}

fn build_cc_family(
    source_root: &Path,
    name: &str,
    lib_name: &str,
    out: &Path,
    cpp: bool,
) -> Result<()> {
    // Prefer a simple Makefile `shared` / `lib` target when present.
    let makefile = source_root.join("Makefile");
    let makefile_lc = source_root.join("makefile");
    if makefile.is_file() || makefile_lc.is_file() {
        let status = Command::new("make")
            .arg(format!("OUT={}", out.display()))
            .arg(format!("LIBNAME={lib_name}"))
            .current_dir(source_root)
            .status()
            .context("spawn make for path/git native build")?;
        if status.success() && out.is_file() {
            return Ok(());
        }
        // Fall through to direct compile when make doesn't produce OUT.
    }

    if source_root.join("CMakeLists.txt").is_file() {
        bail!(
            "path/git `{name}` has CMakeLists.txt but no simple shared-lib recipe.\n\
             Feasible auto-build today: flat .c/.cpp sources or a Makefile that writes $OUT.\n\
             Build the library yourself and point path: at the artifact dir, or add a Makefile."
        );
    }
    if source_root.join("meson.build").is_file() {
        bail!(
            "path/git `{name}` uses meson; rig does not auto-drive meson yet.\n\
             Add a Makefile that builds a shared lib to $OUT, or vendor a prebuilt .so/.dylib."
        );
    }

    let exts: &[&str] = if cpp {
        &["cpp", "cxx", "cc"]
    } else {
        &["c"]
    };
    let mut sources = Vec::new();
    collect_sources(source_root, exts, &mut sources, 3)?;
    if sources.is_empty() {
        bail!(
            "no compilable {} sources found under {} for `{name}`.\n\
             Expected *.{} (depth ≤3), or a Makefile that emits $OUT.",
            if cpp { "C++" } else { "C" },
            source_root.display(),
            exts.join("/"),
        );
    }

    let compiler = if cpp {
        std::env::var("CXX").unwrap_or_else(|_| "c++".into())
    } else {
        std::env::var("CC").unwrap_or_else(|_| "cc".into())
    };
    let mut cmd = Command::new(&compiler);
    cmd.arg("-fPIC").arg("-O2");
    #[cfg(target_os = "macos")]
    {
        cmd.arg("-dynamiclib");
    }
    #[cfg(not(target_os = "macos"))]
    {
        cmd.arg("-shared");
    }
    cmd.arg("-o").arg(out);
    for s in &sources {
        cmd.arg(s);
    }
    let status = cmd
        .status()
        .with_context(|| format!("spawn {compiler} for path/git `{name}`"))?;
    if !status.success() {
        bail!(
            "{compiler} failed building shared lib for `{name}` from {} (status {status}).\n\
             Sources: {}\n\
             Fix compile errors locally, or provide a Makefile that writes $OUT.",
            source_root.display(),
            sources
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

fn build_zig_shared(
    source_root: &Path,
    name: &str,
    lib_name: &str,
    out: &Path,
) -> Result<()> {
    if source_root.join("build.zig").is_file() {
        let status = Command::new("zig")
            .args(["build", "-Doptimize=ReleaseFast"])
            .current_dir(source_root)
            .status()
            .context("spawn zig build for path/git native")?;
        if !status.success() {
            bail!(
                "zig build failed for path/git `{name}` in {} (status {status}).\n\
                 Ensure `zig build` produces a shared library, or simplify to a single .zig file.",
                source_root.display()
            );
        }
        // Prefer zig-out/lib/* then copy.
        let zig_out = source_root.join("zig-out/lib");
        if zig_out.is_dir() {
            for ent in std::fs::read_dir(&zig_out)?.flatten() {
                let p = ent.path();
                let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if n.contains(".so") || n.contains(".dylib") || n.ends_with(".dll") {
                    std::fs::copy(&p, out)
                        .with_context(|| format!("copy {} → {}", p.display(), out.display()))?;
                    return Ok(());
                }
            }
        }
        bail!(
            "zig build for `{name}` succeeded but no shared library found under zig-out/lib.\n\
             Configure build.zig to install a shared library, or use a single root .zig source."
        );
    }

    // Single-file / multi-file: zig build-lib -dynamic
    let mut sources = Vec::new();
    collect_sources(source_root, &["zig"], &mut sources, 2)?;
    // Prefer root-level .zig that isn't build.zig
    sources.retain(|p| {
        p.file_name()
            .and_then(|s| s.to_str())
            .map(|n| n != "build.zig")
            .unwrap_or(true)
    });
    if sources.is_empty() {
        bail!(
            "no .zig sources found under {} for `{name}` (and no build.zig).",
            source_root.display()
        );
    }
    let root_src = sources
        .iter()
        .find(|p| p.parent() == Some(source_root))
        .unwrap_or(&sources[0]);

    let status = Command::new("zig")
        .args(["build-lib", "-dynamic", "-OReleaseFast", "-femit-bin"])
        .arg(out)
        .arg(root_src)
        .current_dir(source_root)
        .status()
        .context("spawn zig build-lib")?;
    if !status.success() {
        bail!(
            "zig build-lib failed for `{name}` from {} (status {status}).\n\
             Tip: export a C ABI with `export fn …` in the Zig source.",
            root_src.display()
        );
    }
    let _ = lib_name; // name encoded in `out`
    Ok(())
}

fn collect_sources(root: &Path, exts: &[&str], out: &mut Vec<PathBuf>, max_depth: usize) -> Result<()> {
    fn walk(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>, depth: usize, max_depth: usize) -> Result<()> {
        if depth > max_depth {
            return Ok(());
        }
        for ent in std::fs::read_dir(dir)? {
            let ent = ent?;
            let path = ent.path();
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" || name == "zig-out" || name == ".zig-cache" {
                continue;
            }
            if path.is_dir() {
                walk(&path, exts, out, depth + 1, max_depth)?;
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
                && exts.contains(&ext)
            {
                out.push(path);
            }
        }
        Ok(())
    }
    walk(root, exts, out, 0, max_depth)
}

fn collect_headers(root: &Path) -> Vec<PathBuf> {
    let mut headers = Vec::new();
    let _ = collect_sources(root, &["h", "hpp", "hxx"], &mut headers, 2);
    headers
}

/// Write a richer C header for a built path/git native lib (includes found headers + link meta).
pub fn write_c_path_native_header(
    out: &Path,
    name: &str,
    dep: &Dependency,
    built: &PathNativeBuild,
    include_names: &[String],
) -> Result<()> {
    let safe = name.replace('-', "_");
    let guard = format!("RIG_{}_PATH_NATIVE_H", safe.to_uppercase());
    let mut body = String::new();
    body.push_str(&format!(
        "/* Auto-generated by rig — built path/git `{}` ({}) → {}.\n\
         * Link: -L{} -l{}\n\
         */\n\
         #ifndef {guard}\n\
         #define {guard}\n\n",
        name,
        dep.ecosystem,
        built.lib_path.display(),
        built.native_rel,
        built.lib_name,
    ));
    for fname in include_names {
        body.push_str(&format!("#include \"{fname}\"\n"));
    }
    if include_names.is_empty() {
        body.push_str(&format!(
            "/* No public headers discovered under {}. */\n\
             /* Declare extern symbols that your .c/.cpp exports. */\n",
            built.source_root.display()
        ));
    }
    body.push_str(&format!(
        "\n\
         #define RIG_NATIVE_LIB_{safe} \"{lib}\"\n\
         #define RIG_NATIVE_DIR_{safe} \"{dir}\"\n\
         #define RIG_SOURCE_ROOT_{safe} \"{src}\"\n\
         \n\
         #endif /* {guard} */\n",
        lib = built.lib_name,
        dir = built.native_rel,
        src = built.source_root.display(),
    ));
    std::fs::write(out, body).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

/// Zig bindings stub that links a built path/git native library.
pub fn write_zig_path_native_bindings(
    out: &Path,
    name: &str,
    dep: &Dependency,
    built: &PathNativeBuild,
) -> Result<()> {
    let safe = name.replace('-', "_");
    let body = format!(
        "//! Auto-generated by rig — Zig ← path/git `{name}` ({eco}).\n\
         //! Native: {lib} @ {dir}\n\
         //! Source: {src}\n\
         //! Do not edit — regenerate with `rig sync` / `rig add`.\n\
         \n\
         pub const package_name = \"{name}\";\n\
         pub const native_lib = \"{lib}\";\n\
         pub const native_dir = \"{dir}\";\n\
         pub const source_root = \"{src}\";\n\
         \n\
         // Link via build.zig (rig patches addLibraryPath / linkSystemLibrary when present).\n\
         // Import C headers with @cImport when available:\n\
         //   const c = @cImport({{ @cInclude(\"{safe}.h\"); }});\n",
        eco = dep.ecosystem,
        lib = built.lib_name,
        dir = built.native_rel,
        src = built.source_root.display(),
    );
    std::fs::write(out, body).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn collect_c_sources_depth() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.c"), "int a(void){return 1;}").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.c"), "int b(void){return 2;}").unwrap();
        let mut srcs = Vec::new();
        collect_sources(dir.path(), &["c"], &mut srcs, 3).unwrap();
        assert_eq!(srcs.len(), 2);
    }
}
