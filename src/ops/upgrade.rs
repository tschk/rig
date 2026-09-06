use crate::cli::commands::AddArgs;
use crate::cli::commands::UpgradeArgs;
use crate::cli::globals::Globals;
use crate::ops::add;
use anyhow::Result;

pub fn run(args: &UpgradeArgs, g: &Globals) -> Result<u8> {
    let ctx = crate::util::load_ctx(g, true)?;
    let names: Vec<String> = if args.packages.is_empty() {
        ctx.manifest.dependencies.keys().cloned().collect()
    } else {
        args.packages.clone()
    };
    if names.is_empty() {
        println!("nothing to upgrade");
        return Ok(0);
    }
    // Re-add without pinning old version (latest)
    let add_args = AddArgs {
        packages: names,
        eco: args.eco.clone(),
        host: None,
        features: None,
        no_default_features: false,
    };
    println!("upgrading…");
    add::run(&add_args, g)
}
