//! Zig package resolution: URL / path / git pins + search hints.
//! Zig has no crates.io-style central registry; be honest about that.

use super::http::{agent, urlencoding_lite};
use super::{PackageSpec, ResolvedPackage};
use anyhow::{Result, bail};

pub fn search(query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
    // Best-effort GitHub hints — not a registry pin source.
    let url = format!(
        "https://api.github.com/search/repositories?q={}+language:Zig&per_page={}",
        urlencoding_lite(query),
        limit.min(30)
    );
    match agent().get(&url).call() {
        Ok(resp) => {
            let json: serde_json::Value = resp.into_json()?;
            let mut out = Vec::new();
            if let Some(arr) = json.get("items").and_then(|i| i.as_array()) {
                for repo in arr.iter().take(limit) {
                    let name = repo
                        .get("full_name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let desc = repo
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let html = repo
                        .get("html_url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() {
                        out.push((name, "git".into(), format!("{desc} ({html})")));
                    }
                }
            }
            Ok(out)
        }
        Err(_) => Ok(Vec::new()),
    }
}

pub fn resolve(
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    if let Some(pkg) =
        super::git_path::resolve_path_or_git("zig", spec, features.clone(), no_default)
    {
        return Ok(pkg);
    }
    if let Some(url) = &spec.url {
        return Ok(ResolvedPackage {
            name: spec.name.clone(),
            ecosystem: "zig".into(),
            version: spec.version_req.clone().unwrap_or_else(|| "url".into()),
            source: format!("zig+{url}"),
            checksum: None,
            git: None,
            path: None,
            url: Some(url.clone()),
            features,
            default_features: if no_default { Some(false) } else { None },
        });
    }

    bail!(
        "zig has no central package registry — pin with path:…, git+…, or an https:// URL\n\
         example: rig add --zig path:./vendor/mylib\n\
         example: rig add --zig git+https://github.com/org/pkg\n\
         example: rig add --zig https://github.com/org/pkg/archive/refs/tags/1.0.0.tar.gz"
    )
}

pub const NO_REGISTRY_MSG: &str = "zig has no central registry — showing GitHub language:Zig hints (not install pins).\n\
     Prefer: rig add --zig path:… | git+… | https://…";
