use crate::cli::commands::SearchArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::resolve::cargo;
use anyhow::Result;

pub fn run(args: &SearchArgs, g: &Globals) -> Result<u8> {
    let eco = args.eco.language().unwrap_or(Language::Rust);
    match eco {
        Language::Rust => {
            let hits = cargo::search(&args.query, args.limit)?;
            if hits.is_empty() {
                println!("no crates matched {:?}", args.query);
                return Ok(0);
            }
            for (name, ver, desc) in hits {
                let d = if desc.len() > 72 {
                    format!("{}…", &desc[..72])
                } else {
                    desc
                };
                println!("{name:24} {ver:10} {d}");
            }
            Ok(0)
        }
        other => {
            if g.verbose {
                eprintln!("search for {other} is best-effort (registry client pending)");
            }
            println!(
                "search ({other}): no registry client yet — try path:/git+ specs with `rig add --{} <pkg>`",
                other.ecosystem()
            );
            println!("query was: {}", args.query);
            Ok(0)
        }
    }
}
