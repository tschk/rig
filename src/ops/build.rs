use crate::cli::commands::BuildArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::expose::{self, stamp, ts_host};
use crate::util;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

pub fn run(args: &BuildArgs, g: &Globals) -> Result<u8> {
    let ctx = util::load_ctx(g, true)?;
    if !stamp::is_fresh(&ctx) {
        if g.verbose {
            eprintln!("# expose stale — resync");
        }
        expose::resync_all(&ctx)?;
    }

    let status = match ctx.host.language {
        Language::Rust => {
            let mut cmd = Command::new("cargo");
            cmd.arg("build").current_dir(&ctx.root);
            for a in &args.args {
                cmd.arg(a);
            }
            cmd.status().context("cargo build")?
        }
        Language::Zig => {
            let mut cmd = Command::new("zig");
            cmd.arg("build").current_dir(&ctx.root);
            for a in &args.args {
                cmd.arg(a);
            }
            cmd.status().context("zig build")?
        }
        Language::TypeScript => {
            // scriptc takes one `--ffi` manifest, so rig supplies the merged one
            // and the host entry point; everything else is passed through.
            let (entry, rest) = match args.args.first() {
                Some(first) if !first.starts_with('-') => (first.clone(), &args.args[1..]),
                _ => (
                    ts_entry(&ctx.root).with_context(|| {
                        "rig build: pass the TypeScript entry file (e.g. `rig build src/main.ts`)"
                    })?,
                    &args.args[..],
                ),
            };
            let mut cmd = Command::new("scriptc");
            cmd.arg("build").arg(entry).current_dir(&ctx.root);
            let manifest = ctx
                .root
                .join(&ctx.manifest.expose.dir)
                .join(ts_host::HOST_MANIFEST);
            if manifest.is_file() {
                cmd.arg("--ffi").arg(&manifest);
            }
            for a in rest {
                cmd.arg(a);
            }
            cmd.status().context("scriptc build")?
        }
        other => {
            bail!("rig build for host={other} not wired yet — build with your native toolchain");
        }
    };

    if status.success() { Ok(0) } else { Ok(3) }
}

/// First conventional scriptc entry point in the host, if any.
fn ts_entry(root: &Path) -> Option<String> {
    ["src/main.ts", "src/index.ts", "main.ts", "index.ts"]
        .into_iter()
        .find(|candidate| root.join(candidate).is_file())
        .map(str::to_string)
}
