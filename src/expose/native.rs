//! Build cargo cdylib façades into `target/rig/<pkg>/`.

use super::shim::{self, ShimArtifacts};
use crate::manifest::Dependency;
use crate::resolve::ResolvedPackage;
use crate::util::AppCtx;
use anyhow::{Context, Result, bail};
use std::process::Command;

pub struct NativeBuild {
    pub artifacts: ShimArtifacts,
    pub lib_path: std::path::PathBuf,
}

/// Ensure shim exists, `cargo build --release`, install into native out dir.
pub fn build_cargo_cdylib(
    ctx: &AppCtx,
    name: &str,
    resolved: Option<&ResolvedPackage>,
    dep: &Dependency,
) -> Result<NativeBuild> {
    let artifacts = shim::ensure_shim(ctx, name, resolved, dep)?;
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(artifacts.shim_dir.join("Cargo.toml"))
        .current_dir(&artifacts.shim_dir)
        .status()
        .context("spawn cargo build for rig shim")?;
    if !status.success() {
        bail!(
            "cargo build failed for shim {} (status {status})",
            artifacts.shim_dir.display()
        );
    }
    let target_dir = artifacts.shim_dir.join("target");
    let lib_path = shim::install_artifacts(&artifacts, &target_dir)?;
    Ok(NativeBuild {
        artifacts,
        lib_path,
    })
}
