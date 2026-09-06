//! Thin dub / code.dlang.org client.

use super::http::{agent, pick_semver, urlencoding_lite};
use super::{PackageSpec, ResolvedPackage};
use anyhow::{Context, Result, bail};

pub fn search(query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
    let url = format!(
        "https://code.dlang.org/api/packages/search?q={}&limit={}",
        urlencoding_lite(query),
        limit
    );
    let resp = agent()
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let json: serde_json::Value = resp.into_json().context("decode dub search JSON")?;
    let mut out = Vec::new();
    if let Some(arr) = json.as_array() {
        for c in arr.iter().take(limit) {
            let name = c
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let ver = c
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let desc = c
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                out.push((name, ver, desc));
            }
        }
    }
    Ok(out)
}

fn fetch_package(name: &str) -> Result<serde_json::Value> {
    let url = format!("https://code.dlang.org/packages/{name}.json");
    let resp = agent()
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    resp.into_json().context("decode dub package JSON")
}

fn versions_from(json: &serde_json::Value) -> Vec<String> {
    json.get("versions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("version").and_then(|x| x.as_str()))
                // Skip dub branch versions like ~master
                .filter(|v| v.as_bytes().first().is_some_and(|b| b.is_ascii_digit()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn resolve(
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    if let Some(pkg) = super::git_path::resolve_path_or_git("d", spec, features.clone(), no_default)
    {
        return Ok(pkg);
    }

    let json = fetch_package(&spec.name)?;
    let name = json
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or(&spec.name)
        .to_string();
    let versions = versions_from(&json);
    if versions.is_empty() {
        bail!("dub package {} has no numbered versions", spec.name);
    }
    let version = pick_semver(&versions, spec.version_req.as_deref())
        .or_else(|| versions.last().cloned())
        .context("could not select dub version")?;

    Ok(ResolvedPackage {
        name,
        ecosystem: "d".into(),
        version,
        source: format!("dub+https://code.dlang.org/packages/{}", spec.name),
        checksum: None,
        git: None,
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
    fn versions_skip_branches() {
        let json = serde_json::json!({
            "versions": [
                {"version": "~master"},
                {"version": "0.9.0"},
                {"version": "1.0.0"}
            ]
        });
        let v = versions_from(&json);
        assert_eq!(v, vec!["0.9.0", "1.0.0"]);
    }
}
