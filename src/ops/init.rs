use crate::cli::commands::InitArgs;
use crate::cli::globals::Globals;
use crate::detect::{Language, detect_host};
use crate::manifest::{self, Host, Manifest, PackageMeta};
use anyhow::{Result, bail};

pub fn run(args: &InitArgs, g: &Globals) -> Result<u8> {
    let cwd = std::env::current_dir()?;
    let path = cwd.join(manifest::MANIFEST_FILE);
    if path.exists() && !args.force {
        bail!("rig.toml already exists (use --force)");
    }

    let host = if let Some(h) = &args.host {
        let language = Language::parse(h)?;
        crate::detect::DetectedHost {
            language,
            root: cwd.clone(),
            marker: None,
        }
    } else {
        detect_host(&cwd)?
    };

    let package = guess_package_name(&cwd, host.language)
        .ok()
        .map(|name| PackageMeta {
            name: Some(name),
            version: Some("0.0.0".into()),
        });

    let m = Manifest {
        host: Host {
            language: host.language.as_str().into(),
            manifest: host.marker.as_ref().map(|p| {
                p.strip_prefix(&cwd)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned()
            }),
            root: None,
        },
        package,
        ..Manifest::default()
    };

    if g.dry_run {
        println!("would write {}", path.display());
        println!("host.language = {}", host.language);
        return Ok(0);
    }

    manifest::save_manifest(&path, &m)?;
    let lock = crate::manifest::Lockfile::default();
    lock.save(&cwd.join(manifest::LOCK_FILE))?;
    println!("initialized {}", path.display());
    println!("host: {}", host.language);
    Ok(0)
}

fn guess_package_name(cwd: &std::path::Path, lang: Language) -> Result<String> {
    if lang == Language::Rust {
        let cargo = cwd.join("Cargo.toml");
        if cargo.is_file() {
            let text = std::fs::read_to_string(cargo)?;
            if let Ok(doc) = text.parse::<toml::Value>()
                && let Some(n) = doc
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
            {
                return Ok(n.to_string());
            }
        }
    }
    Ok(cwd
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".into()))
}
