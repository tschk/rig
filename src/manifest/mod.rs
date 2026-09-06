pub mod lock;
pub mod schema;

pub use lock::Lockfile;
pub use schema::{Dependency, ExposeConfig, ExposeOpts, Host, Manifest, PackageMeta};

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "rig.toml";
pub const LOCK_FILE: &str = "rig.lock";

pub fn find_manifest(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cand = cur.join(MANIFEST_FILE);
        if cand.is_file() {
            return Some(cand);
        }
        if cur.join(".git").exists() {
            return None;
        }
        if !cur.pop() {
            return None;
        }
    }
}

pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let m: Manifest = toml::from_str(&text).context("parse rig.toml")?;
    Ok(m)
}

pub fn save_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let text = toml::to_string_pretty(manifest).context("serialize rig.toml")?;
    let header = "# rig.toml — project-local native dependency manifest\n";
    std::fs::write(path, format!("{header}{text}"))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn project_root_from_manifest(manifest_path: &Path) -> PathBuf {
    manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}
