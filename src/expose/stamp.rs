use crate::util::AppCtx;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;

pub fn stamp_path(ctx: &AppCtx) -> std::path::PathBuf {
    ctx.root.join(".rig/expose-stamp")
}

pub fn compute(ctx: &AppCtx) -> String {
    let mut hasher = Sha256::new();
    let _ = writeln!(hasher, "{}", ctx.manifest.schema_version);
    let _ = writeln!(hasher, "host|{}", ctx.manifest.host.language);
    let _ = writeln!(hasher, "expose_dir|{}", ctx.manifest.expose.dir);
    for (k, v) in &ctx.manifest.dependencies {
        let _ = writeln!(hasher, "{k}|{}|{:?}|{}", v.ecosystem, v.version, v.expose);
    }
    for p in &ctx.lock.package {
        let _ = writeln!(hasher, "lock|{}|{}", p.name, p.version);
    }
    hex::encode(hasher.finalize())
}

pub fn write_stamp(ctx: &AppCtx) -> Result<()> {
    let path = stamp_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, compute(ctx))?;
    Ok(())
}

pub fn is_fresh(ctx: &AppCtx) -> bool {
    let path = stamp_path(ctx);
    match fs::read_to_string(path) {
        Ok(s) => s.trim() == compute(ctx),
        Err(_) => false,
    }
}
