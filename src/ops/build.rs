use crate::cli::commands::BuildArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::expose::{self, stamp};
use crate::util;
use anyhow::{Context, Result, bail};
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
        other => {
            bail!("rig build for host={other} not wired yet — build with your native toolchain");
        }
    };

    if status.success() { Ok(0) } else { Ok(3) }
}
