pub mod edit;
pub mod paths;

use crate::cli::globals::Globals;
use crate::detect::{DetectedHost, detect_host};
use crate::manifest::{self, Lockfile, Manifest};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub struct AppCtx {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub lock_path: PathBuf,
    pub manifest: Manifest,
    pub lock: Lockfile,
    pub host: DetectedHost,
    pub verbose: bool,
    pub dry_run: bool,
    pub yes: bool,
}

pub fn load_ctx(globals: &Globals, require_manifest: bool) -> Result<AppCtx> {
    let cwd = std::env::current_dir().context("cwd")?;
    let manifest_path = match manifest::find_manifest(&cwd) {
        Some(p) => p,
        None if require_manifest => bail!("no rig.toml found — run `rig init` first"),
        None => cwd.join(manifest::MANIFEST_FILE),
    };
    let root = manifest_path
        .parent()
        .unwrap_or(cwd.as_path())
        .to_path_buf();
    let lock_path = root.join(manifest::LOCK_FILE);

    let (manifest, host) = if manifest_path.is_file() {
        let m = manifest::load_manifest(&manifest_path)?;
        let lang = crate::detect::Language::parse(&m.host.language)?;
        let host = DetectedHost {
            language: lang,
            root: root.clone(),
            marker: None,
        };
        (m, host)
    } else {
        let host = detect_host(&cwd)?;
        (Manifest::default(), host)
    };

    let lock = if lock_path.is_file() {
        Lockfile::load(&lock_path).unwrap_or_default()
    } else {
        Lockfile::default()
    };

    Ok(AppCtx {
        root,
        manifest_path,
        lock_path,
        manifest,
        lock,
        host,
        verbose: globals.verbose,
        dry_run: globals.dry_run,
        yes: globals.yes && !globals.ask,
    })
}

pub fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for p in std::env::split_paths(&paths) {
            let cand = p.join(bin);
            if cand.is_file() {
                return Some(cand);
            }
            #[cfg(windows)]
            {
                let cand = p.join(format!("{bin}.exe"));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
        None
    })
}

pub fn vlog(ctx: &AppCtx, msg: &str) {
    if ctx.verbose {
        eprintln!("# {msg}");
    }
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    Ok(())
}
