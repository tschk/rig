use crate::cli::commands::RemoveArgs;
use crate::cli::globals::Globals;
use crate::expose;
use crate::manifest;
use crate::util::{self, edit};
use anyhow::{Result, bail};

pub fn run(args: &RemoveArgs, g: &Globals) -> Result<u8> {
    if args.packages.is_empty() {
        bail!("no packages specified");
    }
    let mut ctx = util::load_ctx(g, true)?;
    for name in &args.packages {
        if !ctx.manifest.dependencies.contains_key(name) {
            eprintln!("warn: {name} not in rig.toml");
            continue;
        }
        if g.dry_run {
            println!("would remove {name}");
            continue;
        }
        ctx.manifest.dependencies.remove(name);
        ctx.lock.remove(name);
        let _ = edit::cargo_remove_dep(&ctx.root.join("Cargo.toml"), name);
        expose::remove_expose_artifacts(&ctx, name)?;
        println!("removed {name}");
    }
    if !g.dry_run {
        manifest::save_manifest(&ctx.manifest_path, &ctx.manifest)?;
        ctx.lock.save(&ctx.lock_path)?;
        expose::resync_all(&ctx)?;
    }
    Ok(0)
}
