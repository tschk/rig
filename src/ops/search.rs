use crate::cli::commands::SearchArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::resolve::{self, SearchOutcome};
use anyhow::Result;

pub fn run(args: &SearchArgs, g: &Globals) -> Result<u8> {
    let eco = args.eco.language().unwrap_or(Language::Rust);
    let outcome = resolve::search(eco, &args.query, args.limit)?;
    match outcome {
        SearchOutcome::Hits(hits) => print_hits(&hits, &args.query),
        SearchOutcome::Hints { note, hits } => {
            println!("{note}");
            if hits.is_empty() {
                if g.verbose {
                    eprintln!("no GitHub hints for {:?}", args.query);
                }
                println!("query was: {}", args.query);
            } else {
                print_hits(&hits, &args.query)?;
            }
            Ok(0)
        }
        SearchOutcome::Unsupported(msg) => {
            println!("{msg}");
            println!("query was: {}", args.query);
            Ok(0)
        }
    }
}

fn print_hits(hits: &[(String, String, String)], query: &str) -> Result<u8> {
    if hits.is_empty() {
        println!("no packages matched {query:?}");
        return Ok(0);
    }
    for (name, ver, desc) in hits {
        let d = if desc.len() > 72 {
            format!("{}…", &desc[..72])
        } else {
            desc.clone()
        };
        println!("{name:28} {ver:12} {d}");
    }
    Ok(0)
}
