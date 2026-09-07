use crate::cli::commands::AddArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::expose;
use crate::manifest::lock::LockedPackage;
use crate::manifest::{self, Dependency, ExposeOpts};
use crate::resolve::{self, PackageSpec};
use crate::util::{self, edit};
use anyhow::Result;

pub fn run(args: &AddArgs, g: &Globals) -> Result<u8> {
    resolve::ensure_packages(&args.packages)?;
    let mut ctx = util::load_ctx(g, true)?;

    if let Some(h) = &args.host {
        ctx.host.language = Language::parse(h)?;
        ctx.manifest.host.language = ctx.host.language.as_str().into();
    }

    let features = args.features.as_ref().map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
    });

    for pkg in &args.packages {
        let spec = PackageSpec::parse(pkg)?;
        let eco = resolve::infer_ecosystem(ctx.host.language, args.eco.language(), &spec)?;
        let resolved = resolve::resolve(eco, &spec, features.clone(), args.no_default_features)?;

        let mut expose_opts = ExposeOpts {
            consumer: Some(ctx.host.language.as_str().into()),
            out: None,
            crate_types: if eco == Language::Rust {
                Some(vec!["cdylib".into(), "rlib".into()])
            } else {
                None
            },
            native: Some(format!(
                "{}/{}",
                ctx.manifest.expose.build_dir, resolved.name
            )),
        };
        let safe = resolved.name.replace('-', "_");
        let out = match ctx.host.language {
            Language::Rust => format!("{}/{safe}.rs", ctx.manifest.expose.dir),
            Language::Zig => format!("{}/{safe}_bindings.zig", ctx.manifest.expose.dir),
            Language::Nim => format!("{}/{safe}.nim", ctx.manifest.expose.dir),
            Language::C | Language::Cpp => format!("{}/{safe}.h", ctx.manifest.expose.dir),
            Language::CSharp => format!("{}/{safe}.cs", ctx.manifest.expose.dir),
            Language::V => format!("{}/{safe}.v", ctx.manifest.expose.dir),
            Language::D => format!("{}/{safe}.d", ctx.manifest.expose.dir),
            Language::Odin => format!("{}/{safe}.odin", ctx.manifest.expose.dir),
            Language::Hare => format!("{}/{safe}.ha", ctx.manifest.expose.dir),
        };
        expose_opts.out = Some(out.clone());

        let dep_git = if resolved.source.starts_with("git+") {
            resolved.git.clone()
        } else {
            None
        };
        let dep = Dependency {
            ecosystem: resolved.ecosystem.clone(),
            version: Some(resolved.version.clone()),
            git: dep_git,
            rev: None,
            path: resolved.path.clone(),
            url: resolved.url.clone(),
            features: resolved.features.clone(),
            default_features: resolved.default_features,
            expose: true,
            expose_opts: Some(expose_opts),
        };

        if g.dry_run {
            println!(
                "would add {}@{} ({}) → expose {}",
                resolved.name, resolved.version, resolved.ecosystem, out
            );
            continue;
        }

        // Wire Cargo.toml for rust host + cargo dep
        if ctx.host.language == Language::Rust && resolved.ecosystem == "cargo" {
            let cargo_toml = ctx.root.join("Cargo.toml");
            if cargo_toml.is_file() {
                let git = if resolved.source.starts_with("git+") {
                    resolved.git.as_deref()
                } else {
                    None
                };
                let ver = if git.is_some() {
                    "*".to_string()
                } else {
                    resolved.version.clone()
                };
                edit::cargo_add_dep(
                    &cargo_toml,
                    &resolved.name,
                    &ver,
                    git,
                    resolved.features.as_deref(),
                    resolved.default_features,
                )?;
            }
        }

        ctx.manifest
            .dependencies
            .insert(resolved.name.clone(), dep.clone());
        ctx.lock.upsert(LockedPackage {
            name: resolved.name.clone(),
            ecosystem: resolved.ecosystem.clone(),
            version: resolved.version.clone(),
            source: Some(resolved.source.clone()),
            checksum: resolved.checksum.clone(),
            features: resolved.features.clone(),
            expose_consumer: Some(ctx.host.language.as_str().into()),
            expose_out: Some(out.clone()),
        });

        expose::expose_resolved(&ctx, &resolved, &dep)?;

        println!(
            "added {} {} ({})",
            resolved.name, resolved.version, resolved.ecosystem
        );
        println!("  expose {out}");
    }

    if !g.dry_run {
        manifest::save_manifest(&ctx.manifest_path, &ctx.manifest)?;
        ctx.lock.save(&ctx.lock_path)?;
    }
    Ok(0)
}
