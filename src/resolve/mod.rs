pub mod cargo;
pub mod dub;
pub mod git_path;
pub mod http;
pub mod nim;
pub mod nuget;
pub mod zig;

use crate::detect::Language;
use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub name: String,
    pub version_req: Option<String>,
    pub git: Option<String>,
    pub path: Option<String>,
    /// Direct package URL (used by Zig / URL pins).
    pub url: Option<String>,
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
                url: None,
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
                url: None,
            });
        }
        if spec.starts_with("https://") || spec.starts_with("http://") {
            let url = spec.to_string();
            let name = url_package_name(&url);
            return Ok(Self {
                name,
                version_req: None,
                git: None,
                path: None,
                url: Some(url),
            });
        }
        if let Some((name, ver)) = spec.split_once('@') {
            return Ok(Self {
                name: name.to_string(),
                version_req: Some(ver.to_string()),
                git: None,
                path: None,
                url: None,
            });
        }
        Ok(Self {
            name: spec.to_string(),
            version_req: None,
            git: None,
            path: None,
            url: None,
        })
    }
}

fn url_package_name(url: &str) -> String {
    // Prefer repo name for GitHub archive URLs: …/org/pkg/archive/refs/tags/x.tar.gz
    if let Some(idx) = url.find("/archive/") {
        let head = &url[..idx];
        if let Some(name) = head.rsplit('/').next()
            && !name.is_empty()
        {
            return name.to_string();
        }
    }
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("dep");
    let stem = last
        .trim_end_matches(".tar.gz")
        .trim_end_matches(".tgz")
        .trim_end_matches(".zip")
        .trim_end_matches(".git");
    if stem.is_empty() || stem.chars().all(|c| c.is_ascii_digit() || c == '.') {
        // Fall back one more segment when last looks like a version
        let segs: Vec<_> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() >= 2 {
            return segs[segs.len() - 2].to_string();
        }
    }
    if stem.is_empty() {
        "dep".into()
    } else {
        stem.to_string()
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
    pub url: Option<String>,
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
        Language::Nim => nim::resolve(spec, features, no_default),
        Language::D => dub::resolve(spec, features, no_default),
        Language::CSharp => nuget::resolve(spec, features, no_default),
        Language::Zig => zig::resolve(spec, features, no_default),
        Language::C | Language::Cpp | Language::V | Language::Odin | Language::Hare => {
            git_path::resolve_path_git_only(eco.ecosystem(), spec, features, no_default)
        }
    }
}

pub fn search(
    eco: Language,
    query: &str,
    limit: usize,
) -> Result<SearchOutcome> {
    match eco {
        Language::Rust => Ok(SearchOutcome::Hits(cargo::search(query, limit)?)),
        Language::Nim => Ok(SearchOutcome::Hits(nim::search(query, limit)?)),
        Language::D => Ok(SearchOutcome::Hits(dub::search(query, limit)?)),
        Language::CSharp => Ok(SearchOutcome::Hits(nuget::search(query, limit)?)),
        Language::Zig => {
            let hits = zig::search(query, limit)?;
            Ok(SearchOutcome::Hints {
                note: zig::NO_REGISTRY_MSG.into(),
                hits,
            })
        }
        Language::C | Language::Cpp | Language::V | Language::Odin | Language::Hare => {
            Ok(SearchOutcome::Unsupported(git_path::search_unsupported(
                eco.ecosystem(),
            )))
        }
    }
}

#[derive(Debug)]
pub enum SearchOutcome {
    Hits(Vec<(String, String, String)>),
    Hints {
        note: String,
        hits: Vec<(String, String, String)>,
    },
    Unsupported(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_spec() {
        let s = PackageSpec::parse(
            "https://github.com/org/pkg/archive/refs/tags/1.0.0.tar.gz",
        )
        .unwrap();
        assert_eq!(s.name, "pkg");
        assert!(s.url.is_some());
    }

    #[test]
    fn parse_git_and_path() {
        let g = PackageSpec::parse("git+https://github.com/a/b.git").unwrap();
        assert_eq!(g.name, "b");
        assert!(g.git.is_some());
        let p = PackageSpec::parse("path:./vendor/libfoo").unwrap();
        assert_eq!(p.name, "libfoo");
        assert_eq!(p.path.as_deref(), Some("./vendor/libfoo"));
    }
}
