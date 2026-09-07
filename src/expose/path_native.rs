//! Build path/git non-cargo deps into `target/rig/<pkg>/` when feasible.
//!
//! Supported ecosystems: c, cpp, zig, nim, v, odin, hare.
//! Build drivers (in order for C/C++): Makefile `$OUT` → CMake → meson → flat sources.
//! Honest errors when a required toolchain is missing.

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

const PATH_GIT_ECOSYSTEMS: &[&str] = &["c", "cpp", "zig", "nim", "v", "odin", "hare"];

/// Resolve source tree for a path/git pin, build a shared library when feasible,
/// install into expose `build_dir`, and return link metadata.
pub fn build_path_git_lib(
    ctx: &AppCtx,
    name: &str,
    dep: &Dependency,
    resolved: Option<&ResolvedPackage>,
) -> Result<PathNativeBuild> {
    let eco = dep.ecosystem.as_str();
    if !PATH_GIT_ECOSYSTEMS.contains(&eco) {
        bail!(
            "path/git native build supports {} (got `{eco}` for `{name}`)",
            PATH_GIT_ECOSYSTEMS.join("/")
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
        "c" => build_cc_family(&source_root, name, &lib_name, &dylib, false)?,
        "nim" => build_nim_shared(&source_root, name, &lib_name, &dylib)?,
        "v" => build_v_shared(&source_root, name, &lib_name, &dylib)?,
        "odin" => build_odin_shared(&source_root, name, &lib_name, &dylib)?,
        "hare" => build_hare_shared(&source_root, name, &lib_name, &dylib)?,
        _ => unreachable!(),
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

fn toolchain_on_path(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        || Command::new(bin)
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        || which_exists(bin)
}

fn which_exists(bin: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {bin} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn require_toolchain(bin: &str, eco: &str, name: &str) -> Result<()> {
    if which_exists(bin) {
        return Ok(());
    }
    bail!(
        "`{bin}` not found on PATH — cannot build path/git `{name}` ({eco}).\n\
         Install the {eco} toolchain, or vendor a prebuilt shared library and point path: at it."
    )
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
        // Fall through when make doesn't produce OUT.
    }

    if source_root.join("CMakeLists.txt").is_file() {
        return build_cmake_shared(source_root, name, lib_name, out);
    }
    if source_root.join("meson.build").is_file() {
        return build_meson_shared(source_root, name, lib_name, out);
    }

    let exts: &[&str] = if cpp { &["cpp", "cxx", "cc"] } else { &["c"] };
    let mut sources = Vec::new();
    collect_sources(source_root, exts, &mut sources, 3)?;
    if sources.is_empty() {
        bail!(
            "no compilable {} sources found under {} for `{name}`.\n\
             Expected *.{} (depth ≤3), a Makefile that emits $OUT, CMakeLists.txt, or meson.build.",
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
             Fix compile errors locally, or provide a Makefile / CMakeLists.txt / meson.build.",
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

/// Drive CMake with BUILD_SHARED_LIBS and copy the produced shared lib to `out`.
fn build_cmake_shared(source_root: &Path, name: &str, lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("cmake", "c/cpp (cmake)", name)?;
    let build_dir = source_root.join(".rig-cmake-build");
    let status = Command::new("cmake")
        .arg("-S")
        .arg(source_root)
        .arg("-B")
        .arg(&build_dir)
        .arg("-DBUILD_SHARED_LIBS=ON")
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg(format!(
            "-DCMAKE_LIBRARY_OUTPUT_DIRECTORY={}",
            out.parent().unwrap_or(out).display()
        ))
        .arg(format!(
            "-DCMAKE_RUNTIME_OUTPUT_DIRECTORY={}",
            out.parent().unwrap_or(out).display()
        ))
        .status()
        .context("spawn cmake configure")?;
    if !status.success() {
        bail!(
            "cmake configure failed for path/git `{name}` in {} (status {status}).\n\
             Ensure CMakeLists.txt can build a shared library with -DBUILD_SHARED_LIBS=ON.\n\
             Partial support: flat .c/.cpp or a Makefile writing $OUT also work.",
            source_root.display()
        );
    }
    let mut build = Command::new("cmake");
    build
        .arg("--build")
        .arg(&build_dir)
        .arg("--config")
        .arg("Release");
    if which_exists("ninja") || build_dir.join("build.ninja").is_file() {
        // parallel by default via cmake --build
    }
    let status = build.status().context("spawn cmake --build")?;
    if !status.success() {
        bail!(
            "cmake --build failed for path/git `{name}` (status {status}).\n\
             Inspect {} and fix the project, or add a Makefile that writes $OUT.",
            build_dir.display()
        );
    }
    if out.is_file() {
        return Ok(());
    }
    if let Some(found) =
        find_shared_lib(&build_dir, Some(lib_name)).or_else(|| find_shared_lib(&build_dir, None))
    {
        std::fs::copy(&found, out)
            .with_context(|| format!("copy {} → {}", found.display(), out.display()))?;
        return Ok(());
    }
    // Also check output directory parent in case CMAKE_*_OUTPUT_DIRECTORY landed beside out.
    if let Some(parent) = out.parent()
        && let Some(found) =
            find_shared_lib(parent, Some(lib_name)).or_else(|| find_shared_lib(parent, None))
        && found != out
    {
        std::fs::copy(&found, out)
            .with_context(|| format!("copy {} → {}", found.display(), out.display()))?;
        return Ok(());
    }
    bail!(
        "cmake build for `{name}` succeeded but no shared library (.so/.dylib/.dll) was found under {}.\n\
         Tip: add `add_library(… SHARED …)` or set BUILD_SHARED_LIBS, or provide a Makefile that writes $OUT.\n\
         Static-only CMake projects are not auto-linked yet.",
        build_dir.display()
    )
}

fn build_meson_shared(source_root: &Path, name: &str, lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("meson", "c/cpp (meson)", name)?;
    if !which_exists("ninja") && !toolchain_on_path("ninja") {
        // meson typically needs ninja
        if !which_exists("ninja") {
            bail!(
                "`meson.build` present for `{name}` but `ninja` not on PATH (required by meson).\n\
                 Install ninja, or add a Makefile that writes $OUT."
            );
        }
    }
    let build_dir = source_root.join(".rig-meson-build");
    if !build_dir.join("build.ninja").is_file() {
        let status = Command::new("meson")
            .args(["setup", "--buildtype=release", "-Ddefault_library=shared"])
            .arg(&build_dir)
            .arg(source_root)
            .status()
            .context("spawn meson setup")?;
        if !status.success() {
            // Retry without default_library (option may not exist).
            let _ = std::fs::remove_dir_all(&build_dir);
            let status = Command::new("meson")
                .args(["setup", "--buildtype=release"])
                .arg(&build_dir)
                .arg(source_root)
                .status()
                .context("spawn meson setup (retry)")?;
            if !status.success() {
                bail!(
                    "meson setup failed for path/git `{name}` in {} (status {status}).\n\
                     Ensure meson.build can produce a shared library, or add a Makefile writing $OUT.",
                    source_root.display()
                );
            }
        }
    }
    let status = Command::new("meson")
        .args(["compile", "-C"])
        .arg(&build_dir)
        .status()
        .context("spawn meson compile")?;
    if !status.success() {
        bail!(
            "meson compile failed for path/git `{name}` (status {status}).\n\
             Inspect {} or provide a Makefile that writes $OUT.",
            build_dir.display()
        );
    }
    if out.is_file() {
        return Ok(());
    }
    if let Some(found) =
        find_shared_lib(&build_dir, Some(lib_name)).or_else(|| find_shared_lib(&build_dir, None))
    {
        std::fs::copy(&found, out)
            .with_context(|| format!("copy {} → {}", found.display(), out.display()))?;
        return Ok(());
    }
    bail!(
        "meson build for `{name}` succeeded but no shared library was found under {}.\n\
         Tip: use `library(..., install: true)` with shared default, or a Makefile writing $OUT.",
        build_dir.display()
    )
}

fn find_shared_lib(root: &Path, prefer_name: Option<&str>) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let _ = walk_shared(root, &mut found, 0, 6);
    if let Some(want) = prefer_name
        && let Some(p) = found.iter().find(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.contains(want))
                .unwrap_or(false)
        })
    {
        return Some(p.clone());
    }
    found.into_iter().next()
}

fn walk_shared(dir: &Path, out: &mut Vec<PathBuf>, depth: usize, max_depth: usize) -> Result<()> {
    if depth > max_depth || !dir.is_dir() {
        return Ok(());
    }
    for ent in std::fs::read_dir(dir)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name == "." || name == ".." || name == ".git" {
            continue;
        }
        if path.is_dir() {
            walk_shared(&path, out, depth + 1, max_depth)?;
        } else if is_shared_lib_name(&name) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_shared_lib_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".dylib")
        || lower.ends_with(".so")
        || lower.contains(".so.")
        || lower.ends_with(".dll")
}

fn build_zig_shared(source_root: &Path, name: &str, lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("zig", "zig", name)?;
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
        let zig_out = source_root.join("zig-out/lib");
        if zig_out.is_dir() {
            for ent in std::fs::read_dir(&zig_out)?.flatten() {
                let p = ent.path();
                let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if is_shared_lib_name(n) {
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

    let mut sources = Vec::new();
    collect_sources(source_root, &["zig"], &mut sources, 2)?;
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

    let emit = format!("-femit-bin={}", out.display());
    let status = Command::new("zig")
        .args(["build-lib", "-dynamic", "-OReleaseFast"])
        .arg(&emit)
        .arg("--name")
        .arg(lib_name)
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
    Ok(())
}

fn build_nim_shared(source_root: &Path, name: &str, lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("nim", "nim", name)?;
    let root = pick_source(source_root, &["nim"], &["src"])?;
    // nim c --app:lib writes libfoo.dylib / libfoo.so next to -o basename rules vary;
    // pass full path via -o:
    let status = Command::new("nim")
        .args([
            "c",
            "--app:lib",
            "--noMain",
            "-d:release",
            "--opt:speed",
            "--nimcache:.rig-nimcache",
        ])
        .arg(format!("-o:{}", out.display()))
        .arg(&root)
        .current_dir(source_root)
        .status()
        .context("spawn nim c --app:lib")?;
    if !status.success() {
        bail!(
            "nim failed building shared lib for `{name}` from {} (status {status}).\n\
             Export `{{.exportc.}}` procs for a C ABI, or fix compile errors.",
            root.display()
        );
    }
    if out.is_file() {
        return Ok(());
    }
    // Nim sometimes drops the lib prefix / places beside source.
    if let Some(found) = find_shared_lib(source_root, Some(lib_name))
        .or_else(|| find_shared_lib(source_root, Some(name)))
        .or_else(|| find_shared_lib(out.parent().unwrap_or(source_root), None))
    {
        if found != out {
            std::fs::copy(&found, out)
                .with_context(|| format!("copy {} → {}", found.display(), out.display()))?;
        }
        return Ok(());
    }
    bail!(
        "nim build for `{name}` reported success but shared lib not at {}.\n\
         Check nim --app:lib output naming on this platform.",
        out.display()
    )
}

fn build_v_shared(source_root: &Path, name: &str, lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("v", "v", name)?;
    let root = pick_source(source_root, &["v"], &["src"])?;
    // `v -shared -o <path>` — on Unix produces the given path when it has an extension.
    let status = Command::new("v")
        .args(["-shared", "-prod", "-o"])
        .arg(out)
        .arg(&root)
        .current_dir(source_root)
        .status()
        .context("spawn v -shared")?;
    if !status.success() {
        bail!(
            "v failed building shared lib for `{name}` from {} (status {status}).\n\
             Use `__global` / `[export_name]` C exports as needed.",
            root.display()
        );
    }
    if out.is_file() {
        return Ok(());
    }
    if let Some(found) = find_shared_lib(source_root, Some(lib_name))
        .or_else(|| find_shared_lib(out.parent().unwrap_or(source_root), None))
    {
        if found != out {
            std::fs::copy(&found, out)?;
        }
        return Ok(());
    }
    bail!(
        "v -shared for `{name}` succeeded but no shared library at {}.",
        out.display()
    )
}

fn build_odin_shared(source_root: &Path, name: &str, lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("odin", "odin", name)?;
    // odin build <pkg> -build-mode:shared -out:<path without requiring extension handling>
    let out_stem = out.with_extension("");
    // Prefer package dir; else a single .odin file's parent.
    let pkg = if source_root.join("main.odin").is_file()
        || source_root
            .read_dir()
            .ok()
            .map(|d| {
                d.flatten().any(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x == "odin")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    {
        source_root.to_path_buf()
    } else if source_root.join("src").is_dir() {
        source_root.join("src")
    } else {
        source_root.to_path_buf()
    };

    let status = Command::new("odin")
        .arg("build")
        .arg(&pkg)
        .arg("-build-mode:shared")
        .arg(format!("-out:{}", out_stem.display()))
        .current_dir(source_root)
        .status()
        .context("spawn odin build -build-mode:shared")?;
    if !status.success() {
        bail!(
            "odin failed building shared lib for `{name}` from {} (status {status}).\n\
             Export procs with `@(export)` for a C ABI.",
            pkg.display()
        );
    }
    if out.is_file() {
        return Ok(());
    }
    // Odin may append platform suffix to -out stem.
    if let Some(found) = find_shared_lib(out.parent().unwrap_or(source_root), Some(lib_name))
        .or_else(|| find_shared_lib(source_root, Some(lib_name)))
        .or_else(|| find_shared_lib(out.parent().unwrap_or(source_root), None))
    {
        if found != out {
            std::fs::copy(&found, out)?;
        }
        return Ok(());
    }
    bail!(
        "odin build for `{name}` succeeded but shared lib not at {}.",
        out.display()
    )
}

fn build_hare_shared(source_root: &Path, name: &str, _lib_name: &str, out: &Path) -> Result<()> {
    require_toolchain("hare", "hare", name)?;
    let root = pick_source(source_root, &["ha"], &["src"])?;
    let status = Command::new("hare")
        .args(["build", "-o"])
        .arg(out)
        .arg(&root)
        .current_dir(source_root)
        .status()
        .context("spawn hare build")?;
    if !status.success() {
        bail!(
            "hare build failed for `{name}` from {} (status {status}).\n\
             Note: Hare shared-lib support is toolchain-dependent; prefer exporting a C ABI object.",
            root.display()
        );
    }
    if !out.is_file() {
        bail!(
            "hare build for `{name}` succeeded but output missing at {}.\n\
             Hare path/git shared-lib drive is best-effort; vendor a .so if needed.",
            out.display()
        );
    }
    Ok(())
}

fn pick_source(root: &Path, exts: &[&str], subdirs: &[&str]) -> Result<PathBuf> {
    // Prefer root-level source matching package-ish names.
    let mut sources = Vec::new();
    collect_sources(root, exts, &mut sources, 2)?;
    if sources.is_empty() {
        bail!(
            "no *.{} sources found under {} (depth ≤2).",
            exts.join("/"),
            root.display()
        );
    }
    if let Some(p) = sources.iter().find(|p| p.parent() == Some(root)) {
        return Ok(p.clone());
    }
    for sub in subdirs {
        let d = root.join(sub);
        if let Some(p) = sources.iter().find(|p| p.parent() == Some(&d)) {
            return Ok(p.clone());
        }
    }
    Ok(sources[0].clone())
}

fn collect_sources(
    root: &Path,
    exts: &[&str],
    out: &mut Vec<PathBuf>,
    max_depth: usize,
) -> Result<()> {
    fn walk(
        dir: &Path,
        exts: &[&str],
        out: &mut Vec<PathBuf>,
        depth: usize,
        max_depth: usize,
    ) -> Result<()> {
        if depth > max_depth {
            return Ok(());
        }
        for ent in std::fs::read_dir(dir)? {
            let ent = ent?;
            let path = ent.path();
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || name == "target"
                || name == "zig-out"
                || name == ".zig-cache"
                || name == ".rig-cmake-build"
                || name == ".rig-meson-build"
                || name == ".rig-nimcache"
            {
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
             /* Declare extern symbols that your sources export. */\n",
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
    let exports = crate::expose::c_header_scan::scan_headers(&built.headers);
    let mut body = format!(
        "//! Auto-generated by rig — Zig ← path/git `{name}` ({eco}).\n//! Native: {lib} @ {dir}\n//! Source: {src}\n//! Do not edit — regenerate with `rig sync` / `rig add`.\n\npub const package_name = \"{name}\";\npub const native_lib = \"{lib}\";\npub const native_dir = \"{dir}\";\npub const source_root = \"{src}\";\n\n// Link via build.zig (rig patches addLibraryPath / linkSystemLibrary when present).\n",
        eco = dep.ecosystem,
        lib = built.lib_name,
        dir = built.native_rel,
        src = built.source_root.display(),
    );
    if exports.is_empty() {
        body.push_str(&format!(
            "// No simple C prototypes discovered.\n// Import C headers with @cImport when available:\n//   const c = @cImport({{ @cInclude(\"{safe}.h\"); }});\n"
        ));
    } else {
        body.push_str("// Discovered C ABI surface:\n");
        body.push_str(&crate::expose::surface::emit_zig_externs(&exports));
    }
    std::fs::write(out, body).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

pub fn path_native_meta_comment(name: &str, dep: &Dependency, built: &PathNativeBuild) -> String {
    format!(
        "path/git `{name}` ({eco}) → {lib} @ {dir} (src: {src})",
        eco = dep.ecosystem,
        lib = built.lib_name,
        dir = built.native_rel,
        src = built.source_root.display(),
    )
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

    #[test]
    fn cmake_simple_shared_builds_when_toolchain_present() {
        if !which_exists("cmake") || !which_exists("cc") {
            eprintln!("skip: cmake/cc missing");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.16)\n\
             project(demo C)\n\
             add_library(demo SHARED demo.c)\n\
             set_target_properties(demo PROPERTIES OUTPUT_NAME \"demo_native\")\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("demo.c"),
            "int demo_add(int a, int b) { return a + b; }\n",
        )
        .unwrap();
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let dylib = shared_lib_path(&out, "demo_native");
        build_cmake_shared(dir.path(), "demo", "demo_native", &dylib).unwrap();
        assert!(dylib.is_file(), "expected {}", dylib.display());
    }

    #[test]
    fn meson_simple_shared_builds_when_toolchain_present() {
        if !which_exists("meson") || !which_exists("ninja") || !which_exists("cc") {
            eprintln!("skip: meson/ninja/cc missing");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("meson.build"),
            "project('demo', 'c')\n\
             shared_library('demo_native', 'demo.c')\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("demo.c"),
            "int demo_add(int a, int b) { return a + b; }\n",
        )
        .unwrap();
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let dylib = shared_lib_path(&out, "demo_native");
        build_meson_shared(dir.path(), "demo", "demo_native", &dylib).unwrap();
        assert!(dylib.is_file(), "expected {}", dylib.display());
    }

    #[test]
    fn hare_missing_toolchain_is_honest() {
        if which_exists("hare") {
            eprintln!("skip: hare present");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("main.ha"),
            "export fn add(a: int, b: int) int = a + b;\n",
        )
        .unwrap();
        let out = dir.path().join("libx.so");
        let err = build_hare_shared(dir.path(), "x", "x_native", &out).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("hare"), "{msg}");
        assert!(msg.contains("not found") || msg.contains("PATH"), "{msg}");
    }
}
