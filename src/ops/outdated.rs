use crate::cli::commands::OutdatedArgs;
use crate::cli::globals::Globals;
use crate::resolve::cargo;
use crate::util;
use anyhow::Result;

pub fn run(_args: &OutdatedArgs, g: &Globals) -> Result<u8> {
    let ctx = util::load_ctx(g, true)?;
    let mut any = false;
    for (name, dep) in &ctx.manifest.dependencies {
        if dep.ecosystem != "cargo" {
            continue;
        }
        let current = dep.version.as_deref().unwrap_or("?");
        match cargo::latest_version(name) {
            Ok(latest) if latest != current && current != "*" => {
                println!("{name:20} {current:12} → {latest}");
                any = true;
            }
            Ok(_) => {}
            Err(err) => {
                if g.verbose {
                    eprintln!("# skip {name}: {err:#}");
                }
            }
        }
    }
    if !any {
        println!("all cargo deps up to date (or unpinned)");
    }
    Ok(0)
}
