use crate::cli::commands::InfoArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::resolve::{self, PackageSpec};
use crate::util;
use anyhow::Result;

pub fn run(args: &InfoArgs, g: &Globals) -> Result<u8> {
    if let Ok(ctx) = util::load_ctx(g, false)
        && let Some(dep) = ctx.manifest.dependencies.get(&args.package)
    {
        println!("name:       {}", args.package);
        println!("ecosystem:  {}", dep.ecosystem);
        println!("version:    {}", dep.version.as_deref().unwrap_or("?"));
        if let Some(git) = &dep.git {
            println!("git:        {git}");
        }
        if let Some(path) = &dep.path {
            println!("path:       {path}");
        }
        if let Some(url) = &dep.url {
            println!("url:        {url}");
        }
        println!("expose:     {}", dep.expose);
        if let Some(out) = dep.expose_opts.as_ref().and_then(|o| o.out.as_ref()) {
            println!("expose.out: {out}");
        }
        if let Some(locked) = ctx.lock.package.iter().find(|p| p.name == args.package) {
            println!("locked:     {}", locked.version);
            if let Some(src) = &locked.source {
                println!("source:     {src}");
            }
            if let Some(sum) = &locked.checksum {
                println!("checksum:   {sum}");
            }
        }
        return Ok(0);
    }

    let eco = args.eco.language().unwrap_or(Language::Rust);
    let spec = PackageSpec::parse(&args.package)?;
    let resolved = resolve::resolve(eco, &spec, None, false)?;
    println!("name:      {}", resolved.name);
    println!("ecosystem: {}", resolved.ecosystem);
    println!("version:   {}", resolved.version);
    println!("source:    {}", resolved.source);
    if let Some(git) = &resolved.git {
        println!("git:       {git}");
    }
    if let Some(path) = &resolved.path {
        println!("path:      {path}");
    }
    if let Some(url) = &resolved.url {
        println!("url:       {url}");
    }
    if let Some(c) = resolved.checksum {
        println!("checksum:  {c}");
    }
    Ok(0)
}
