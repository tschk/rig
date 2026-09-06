use crate::cli::commands::SyncArgs;
use crate::cli::globals::Globals;
use crate::expose;
use crate::util::{self, edit};
use anyhow::Result;

pub fn run(_args: &SyncArgs, g: &Globals) -> Result<u8> {
    let ctx = util::load_ctx(g, true)?;
    if g.dry_run {
        println!("would sync expose + cargo deps from rig.lock / rig.toml");
        return Ok(0);
    }
    // Ensure cargo deps match for rust host
    if ctx.host.language == crate::detect::Language::Rust {
        let cargo_toml = ctx.root.join("Cargo.toml");
        if cargo_toml.is_file() {
            for (name, dep) in &ctx.manifest.dependencies {
                if dep.ecosystem != "cargo" {
                    continue;
                }
                let ver = dep.version.as_deref().unwrap_or("*");
                let git = dep.git.as_deref();
                edit::cargo_add_dep(
                    &cargo_toml,
                    name,
                    ver,
                    if dep.git.is_some() && dep.version.as_deref() == Some("git") {
                        git
                    } else {
                        None
                    },
                    dep.features.as_deref(),
                    dep.default_features,
                )?;
            }
        }
    }
    let outs = expose::resync_all(&ctx)?;
    println!("synced {} expose artifact(s)", outs.len());
    for o in outs {
        println!(
            "  {}",
            o.out.strip_prefix(&ctx.root).unwrap_or(&o.out).display()
        );
    }
    Ok(0)
}
