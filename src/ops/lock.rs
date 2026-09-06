use crate::cli::commands::LockArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::manifest::lock::LockedPackage;
use crate::resolve::{self, PackageSpec};
use crate::util;
use anyhow::Result;

pub fn run(_args: &LockArgs, g: &Globals) -> Result<u8> {
    let mut ctx = util::load_ctx(g, true)?;
    ctx.lock.package.clear();
    for (name, dep) in &ctx.manifest.dependencies {
        let eco = Language::parse(&dep.ecosystem).unwrap_or(Language::Rust);
        let mut spec = PackageSpec {
            name: name.clone(),
            version_req: dep.version.clone(),
            git: dep.git.clone(),
            path: dep.path.clone(),
        };
        // Prefer exact locked resolve
        let resolved = resolve::resolve(
            eco,
            &spec,
            dep.features.clone(),
            dep.default_features == Some(false),
        )?;
        let _ = &mut spec;
        ctx.lock.upsert(LockedPackage {
            name: resolved.name,
            ecosystem: resolved.ecosystem,
            version: resolved.version,
            source: Some(resolved.source),
            checksum: resolved.checksum,
            features: resolved.features,
            expose_consumer: dep.expose_opts.as_ref().and_then(|o| o.consumer.clone()),
            expose_out: dep.expose_opts.as_ref().and_then(|o| o.out.clone()),
        });
    }
    if g.dry_run {
        println!(
            "would write {} ({} packages)",
            ctx.lock_path.display(),
            ctx.lock.package.len()
        );
        return Ok(0);
    }
    ctx.lock.save(&ctx.lock_path)?;
    println!(
        "wrote {} ({} packages)",
        ctx.lock_path.display(),
        ctx.lock.package.len()
    );
    Ok(0)
}
