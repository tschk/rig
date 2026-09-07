//! Thin NuGet v3 client.

use super::http::{agent, pick_semver, urlencoding_lite};
use super::{PackageSpec, ResolvedPackage};
use anyhow::{Context, Result, bail};
use std::sync::OnceLock;

static SEARCH_ENDPOINT: OnceLock<String> = OnceLock::new();

fn search_endpoint() -> String {
    SEARCH_ENDPOINT
        .get_or_init(|| {
            discover_search_endpoint()
                .unwrap_or_else(|_| "https://azuresearch-usnc.nuget.org/query".to_string())
        })
        .clone()
}

fn discover_search_endpoint() -> Result<String> {
    let url = "https://api.nuget.org/v3/index.json";
    let resp = agent()
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let json: serde_json::Value = resp.into_json()?;
    let resources = json
        .get("resources")
        .and_then(|r| r.as_array())
        .context("missing nuget resources")?;
    // Prefer a typed SearchQueryService entry
    for r in resources {
        let ty = r.get("@type").and_then(|t| t.as_str()).unwrap_or("");
        if ty.starts_with("SearchQueryService")
            && let Some(id) = r.get("@id").and_then(|i| i.as_str())
        {
            return Ok(id.to_string());
        }
    }
    bail!("nuget SearchQueryService not found")
}

pub fn search(query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
    let url = format!(
        "{}?q={}&take={}",
        search_endpoint(),
        urlencoding_lite(query),
        limit
    );
    let resp = agent()
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let json: serde_json::Value = resp.into_json().context("decode nuget search JSON")?;
    let mut out = Vec::new();
    if let Some(arr) = json.get("data").and_then(|d| d.as_array()) {
        for c in arr.iter().take(limit) {
            let name = c
                .get("id")
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

fn flat_versions(id: &str) -> Result<Vec<String>> {
    let lower = id.to_ascii_lowercase();
    let url = format!("https://api.nuget.org/v3-flatcontainer/{lower}/index.json");
    let resp = agent()
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let json: serde_json::Value = resp.into_json()?;
    Ok(json
        .get("versions")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

pub fn resolve(
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    if let Some(pkg) =
        super::git_path::resolve_path_or_git("csharp", spec, features.clone(), no_default)
    {
        return Ok(pkg);
    }

    // Prefer exact match from search (gives a good stable version)
    let hits = search(&spec.name, 20)?;
    let exact = hits
        .iter()
        .find(|(id, _, _)| id.eq_ignore_ascii_case(&spec.name));

    let (name, version) = if let Some(req) = spec.version_req.as_deref() {
        let versions = flat_versions(&spec.name)?;
        if versions.is_empty() {
            bail!("nuget package not found: {}", spec.name);
        }
        let ver = pick_semver(&versions, Some(req)).context("could not select nuget version")?;
        let name = exact
            .map(|(n, _, _)| n.clone())
            .unwrap_or_else(|| spec.name.clone());
        (name, ver)
    } else if let Some((id, ver, _)) = exact {
        (id.clone(), ver.clone())
    } else {
        let versions = flat_versions(&spec.name)?;
        if versions.is_empty() {
            bail!("nuget package not found: {}", spec.name);
        }
        let ver = pick_semver(&versions, None).context("could not select nuget version")?;
        (spec.name.clone(), ver)
    };

    Ok(ResolvedPackage {
        name,
        ecosystem: "csharp".into(),
        version,
        source: "nuget+https://api.nuget.org/v3/index.json".into(),
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
    use super::super::http::pick_semver;

    #[test]
    fn nuget_style_pick_skips_prerelease() {
        let v = vec!["13.0.3".into(), "13.0.4".into(), "13.0.5-beta1".into()];
        assert_eq!(pick_semver(&v, None).as_deref(), Some("13.0.4"));
    }
}
