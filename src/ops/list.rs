use crate::cli::commands::ListArgs;
use crate::cli::globals::Globals;
use crate::util;
use anyhow::Result;

pub fn run(args: &ListArgs, g: &Globals) -> Result<u8> {
    let ctx = util::load_ctx(g, true)?;
    if ctx.manifest.dependencies.is_empty() {
        println!("(no dependencies)");
        return Ok(0);
    }
    for (name, dep) in &ctx.manifest.dependencies {
        if let Some(q) = &args.query
            && !name.contains(q)
            && !dep.ecosystem.contains(q)
        {
            continue;
        }
        let ver = dep.version.as_deref().unwrap_or("?");
        let expose = if dep.expose { "expose" } else { "no-expose" };
        let locked = ctx
            .lock
            .package
            .iter()
            .find(|p| p.name == *name)
            .map(|p| p.version.as_str())
            .unwrap_or("-");
        println!(
            "{name:20} {ver:12} {:8} lock={locked:12} {expose}",
            dep.ecosystem
        );
    }
    Ok(0)
}
