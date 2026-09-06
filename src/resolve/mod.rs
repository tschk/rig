pub mod cargo;
pub mod git_path;

use crate::detect::Language;
use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub name: String,
    pub version_req: Option<String>,
    pub git: Option<String>,
    pub path: Option<String>,
}

impl PackageSpec {
    pub fn parse(spec: &str) -> Result<Self> {
        if let Some(rest) = spec.strip_prefix("git+") {
            let name = rest
                .rsplit('/')
                .next()
                .unwrap_or("dep")
                .trim_end_matches(".git")
                .to_string();
            return Ok(Self {
                name,
                version_req: None,
                git: Some(rest.to_string()),
                path: None,
            });
        }
        if let Some(path) = spec.strip_prefix("path:") {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "dep".into());
            return Ok(Self {
                name,
                version_req: None,
                git: None,
                path: Some(path.to_string()),
            });
        }
        if let Some((name, ver)) = spec.split_once('@') {
            return Ok(Self {
                name: name.to_string(),
                version_req: Some(ver.to_string()),
                git: None,
                path: None,
            });
        }
        Ok(Self {
            name: spec.to_string(),
            version_req: None,
            git: None,
            path: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub ecosystem: String,
    pub version: String,
    pub source: String,
    pub checksum: Option<String>,
    pub git: Option<String>,
    pub path: Option<String>,
    pub features: Option<Vec<String>>,
    pub default_features: Option<bool>,
}

pub fn resolve(
    eco: Language,
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    match eco {
        Language::Rust => cargo::resolve(spec, features, no_default),
        other => {
            if let Some(git) = &spec.git {
                return Ok(ResolvedPackage {
                    name: spec.name.clone(),
                    ecosystem: other.ecosystem().into(),
                    version: spec.version_req.clone().unwrap_or_else(|| "git".into()),
                    source: format!("git+{git}"),
                    checksum: None,
                    git: Some(git.clone()),
                    path: None,
                    features: features.clone(),
                    default_features: if no_default { Some(false) } else { None },
                });
            }
            if let Some(path) = &spec.path {
                return Ok(ResolvedPackage {
                    name: spec.name.clone(),
                    ecosystem: other.ecosystem().into(),
                    version: "path".into(),
                    source: format!("path:{path}"),
                    checksum: None,
                    git: None,
                    path: Some(path.clone()),
                    features,
                    default_features: if no_default { Some(false) } else { None },
                });
            }
            // Registry stubs: pin requested version or "*"
            let ver = spec.version_req.clone().unwrap_or_else(|| "*".into());
            Ok(ResolvedPackage {
                name: spec.name.clone(),
                ecosystem: other.ecosystem().into(),
                version: ver.clone(),
                source: format!("{}+{}", other.ecosystem(), spec.name),
                checksum: None,
                git: None,
                path: None,
                features,
                default_features: if no_default { Some(false) } else { None },
            })
        }
    }
}

pub fn infer_ecosystem(
    host: Language,
    flag: Option<Language>,
    spec: &PackageSpec,
) -> Result<Language> {
    if let Some(f) = flag {
        return Ok(f);
    }
    if spec
        .git
        .as_deref()
        .is_some_and(|g| g.contains("github.com/tschk/rotary"))
        || spec.name == "rx4"
        || spec.name == "rotary"
    {
        return Ok(Language::Rust);
    }
    match host {
        Language::Rust => Ok(Language::Rust),
        other => Ok(other),
    }
}

pub fn ensure_packages(pkgs: &[String]) -> Result<()> {
    if pkgs.is_empty() {
        bail!("no packages specified");
    }
    Ok(())
}
