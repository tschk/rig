use crate::cli::commands::AddArgs;
use crate::cli::commands::UpgradeArgs;
use crate::cli::ecosystem::EcosystemArgs;
use crate::cli::globals::Globals;
use crate::detect::Language;
use crate::ops::add;
use crate::resolve;
use crate::util;
use anyhow::Result;

pub fn run(args: &UpgradeArgs, g: &Globals) -> Result<u8> {
    let ctx = util::load_ctx(g, true)?;
    let names: Vec<String> = if args.packages.is_empty() {
        ctx.manifest.dependencies.keys().cloned().collect()
    } else {
        args.packages.clone()
    };
    if names.is_empty() {
        println!("nothing to upgrade");
        return Ok(0);
    }

    let mut upgraded = 0usize;
    let mut skipped = 0usize;

    for name in &names {
        let Some(dep) = ctx.manifest.dependencies.get(name) else {
            eprintln!("warn: `{name}` not in rig.toml — skip");
            skipped += 1;
            continue;
        };

        let eco_lang = Language::parse(&dep.ecosystem).unwrap_or(Language::Rust);
        let mut eco = if args.eco.language().is_some() {
            args.eco.clone()
        } else {
            eco_from_lang(eco_lang)
        };

        let (pkg_spec, features, no_default_features) = if let Some(path) = &dep.path {
            println!("{name:20} path       → path:{path} (re-expose)");
            (
                format!("path:{path}"),
                dep.features.as_ref().map(|f| f.join(",")),
                dep.default_features == Some(false),
            )
        } else if let Some(git) = &dep.git {
            println!("{name:20} git        → git+{git} (re-expose)");
            (
                format!("git+{git}"),
                dep.features.as_ref().map(|f| f.join(",")),
                dep.default_features == Some(false),
            )
        } else if dep.ecosystem == "cargo" {
            let current = dep.version.as_deref().unwrap_or("*");
            let latest = match resolve::cargo::latest_version(name) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("warn: cannot resolve latest for `{name}`: {err:#}");
                    skipped += 1;
                    continue;
                }
            };
            if current != "*" && current == latest {
                println!("{name:20} {current:12} (already latest)");
                skipped += 1;
                continue;
            }
            println!("{name:20} {current:12} → {latest}");
            eco = eco_from_lang(Language::Rust);
            (
                format!("{name}@{latest}"),
                dep.features.as_ref().map(|f| f.join(",")),
                dep.default_features == Some(false),
            )
        } else {
            let current = dep.version.as_deref().unwrap_or("?");
            println!(
                "{name:20} {current:12} → (re-resolve {})",
                dep.ecosystem
            );
            (
                name.clone(),
                dep.features.as_ref().map(|f| f.join(",")),
                dep.default_features == Some(false),
            )
        };

        let add_args = AddArgs {
            packages: vec![pkg_spec],
            eco,
            host: None,
            features,
            no_default_features,
        };
        let code = add::run(&add_args, g)?;
        if code != 0 {
            return Ok(code);
        }
        upgraded += 1;
    }

    if upgraded == 0 {
        println!("nothing to upgrade ({skipped} already current/skipped)");
    } else {
        println!("upgraded {upgraded} package(s) ({skipped} skipped)");
    }
    Ok(0)
}

fn eco_from_lang(lang: Language) -> EcosystemArgs {
    let mut eco = EcosystemArgs::default();
    match lang {
        Language::Rust => eco.cargo = true,
        Language::Zig => eco.zig = true,
        Language::Nim => eco.nim = true,
        Language::C => eco.c = true,
        Language::Cpp => eco.cpp = true,
        Language::V => eco.vlang = true,
        Language::D => eco.dlang = true,
        Language::Odin => eco.odin = true,
        Language::Hare => eco.hare = true,
        Language::CSharp => eco.csharp = true,
    }
    eco
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eco_from_lang_sets_cargo() {
        let e = eco_from_lang(Language::Rust);
        assert!(e.cargo);
        assert_eq!(e.language(), Some(Language::Rust));
    }

    #[test]
    fn eco_from_lang_sets_c() {
        let e = eco_from_lang(Language::C);
        assert!(e.c);
    }
}
