//! Thin nimble directory client (packages.json index).

use super::http::agent;
use super::{PackageSpec, ResolvedPackage};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const PACKAGES_URL: &str =
    "https://raw.githubusercontent.com/nim-lang/packages/master/packages.json";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone)]
struct NimPkg {
    name: String,
    url: String,
    description: String,
    method: String,
    tags: Vec<String>,
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("rig").join("nim-packages.json"))
}

fn load_index() -> Result<Vec<NimPkg>> {
    if let Some(path) = cache_path()
        && path.is_file()
        && let Ok(meta) = fs::metadata(&path)
        && let Ok(modified) = meta.modified()
        && let Ok(age) = SystemTime::now().duration_since(modified)
        && age < CACHE_TTL
        && let Ok(text) = fs::read_to_string(&path)
        && let Ok(pkgs) = parse_index(&text)
    {
        return Ok(pkgs);
    }

    let resp = agent()
        .get(PACKAGES_URL)
        .call()
        .with_context(|| format!("GET {PACKAGES_URL}"))?;
    let text = resp.into_string().context("read nim packages.json")?;
    let pkgs = parse_index(&text)?;
    if let Some(path) = cache_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, &text);
    }
    Ok(pkgs)
}

fn parse_index(text: &str) -> Result<Vec<NimPkg>> {
    let json: serde_json::Value = serde_json::from_str(text).context("decode nim packages.json")?;
    let arr = json.as_array().context("nim packages.json not an array")?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let url = item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let method = item
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("git")
            .to_string();
        let tags = item
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(NimPkg {
            name,
            url,
            description,
            method,
            tags,
        });
    }
    Ok(out)
}

pub fn search(query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
    let q = query.to_ascii_lowercase();
    let pkgs = load_index()?;
    let mut hits = Vec::new();
    for p in pkgs {
        let hay = format!(
            "{} {} {}",
            p.name.to_ascii_lowercase(),
            p.description.to_ascii_lowercase(),
            p.tags.join(" ").to_ascii_lowercase()
        );
        if hay.contains(&q) {
            let ver = if p.method == "git" {
                "git".to_string()
            } else {
                String::new()
            };
            hits.push((p.name, ver, p.description));
            if hits.len() >= limit {
                break;
            }
        }
    }
    Ok(hits)
}

pub fn resolve(
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    if let Some(pkg) = super::git_path::resolve_path_or_git("nim", spec, features.clone(), no_default)
    {
        return Ok(pkg);
    }

    let pkgs = load_index()?;
    let needle = spec.name.to_ascii_lowercase();
    let found = pkgs
        .into_iter()
        .find(|p| p.name.to_ascii_lowercase() == needle)
        .with_context(|| format!("nimble package not found: {}", spec.name))?;

    let version = spec
        .version_req
        .clone()
        .unwrap_or_else(|| "git".into());
    let git = if found.method == "git" && !found.url.is_empty() {
        Some(found.url.clone())
    } else {
        None
    };

    Ok(ResolvedPackage {
        name: found.name,
        ecosystem: "nim".into(),
        version,
        source: format!("nimble+{}", found.url),
        checksum: None,
        git,
        path: None,
        url: None,
        features,
        default_features: if no_default { Some(false) } else { None },
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_index() {
        let text = r#"[{"name":"jester","url":"https://github.com/dom96/jester","method":"git","tags":["web"],"description":"A sinatra-like web framework for Nim."}]"#;
        let pkgs = parse_index(text).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "jester");
    }
}
