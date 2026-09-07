use crate::cli::commands::CheckArgs;
use crate::cli::globals::Globals;
use crate::expose::{self, stamp};
use crate::util::{self, which};
use anyhow::Result;

pub fn run(args: &CheckArgs, g: &Globals) -> Result<u8> {
    let mut status = 0u8;
    let mut ok = |label: &str, good: bool, detail: &str| {
        if good {
            println!("ok    {label} {detail}");
        } else {
            println!("FAIL  {label} {detail}");
            status = 2;
        }
    };

    ok(
        "rustc",
        which("rustc").is_some(),
        &which("rustc")
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    );
    ok(
        "cargo",
        which("cargo").is_some(),
        &which("cargo")
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    );

    match util::load_ctx(g, false) {
        Ok(ctx) if ctx.manifest_path.is_file() => {
            ok(
                "rig.toml",
                true,
                &format!(
                    "host={} deps={}",
                    ctx.manifest.host.language,
                    ctx.manifest.dependencies.len()
                ),
            );
            let fresh = stamp::is_fresh(&ctx);
            if !fresh && args.fix {
                expose::resync_all(&ctx)?;
                ok("expose", stamp::is_fresh(&ctx), "(repaired)");
            } else {
                ok(
                    "expose",
                    fresh || ctx.manifest.dependencies.is_empty(),
                    if fresh {
                        "fresh"
                    } else {
                        "stale — rig sync / rig check --fix"
                    },
                );
            }
            if args.full {
                for lang in ["zig", "nim", "v", "odin", "hare", "dmd", "dotnet"] {
                    let present = which(lang).is_some();
                    println!(
                        "{} {lang:8} {}",
                        if present { "ok   " } else { "skip " },
                        which(lang)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "not on PATH".into())
                    );
                }
            }
        }
        _ => {
            println!("skip  rig.toml (not found — run rig init)");
        }
    }

    // equilibrium-ffi crate availability is optional; note it
    println!("note  equilibrium-ffi: https://github.com/tschk/equilibrium");
    Ok(status)
}
