use super::http::{agent, urlencoding_lite};
use super::{PackageSpec, ResolvedPackage};
use crate::util::paths::is_valid_crate_name;
use anyhow::{Context, Result, bail};

/// Resolve a cargo crate via crates.io API.
pub fn resolve(
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    if let Some(path) = &spec.path {
        return Ok(ResolvedPackage {
            name: spec.name.clone(),
            ecosystem: "cargo".into(),
            version: "path".into(),
            source: format!("path:{path}"),
            checksum: None,
            git: None,
            path: Some(path.clone()),
            url: None,
            features,
            default_features: if no_default { Some(false) } else { None },
        });
    }
    if let Some(git) = &spec.git {
        return Ok(ResolvedPackage {
            name: crate_name_for(spec),
            ecosystem: "cargo".into(),
            version: "git".into(),
            source: format!("git+{git}"),
            checksum: None,
            git: Some(git.clone()),
            path: None,
            url: None,
            features,
            default_features: if no_default { Some(false) } else { None },
        });
    }

    let name = crate_name_for(spec);
    if !is_valid_crate_name(&name) {
        bail!("invalid crate name `{name}`");
    }

    let mut resolved = fetch_crates_io(&name, spec.version_req.as_deref())?;
    resolved.features = features;
    resolved.default_features = if no_default { Some(false) } else { None };
    Ok(resolved)
}

fn crate_name_for(spec: &PackageSpec) -> String {
    spec.name.clone()
}

fn fetch_crates_io(name: &str, version_req: Option<&str>) -> Result<ResolvedPackage> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let resp = agent()
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let json: serde_json::Value = resp.into_json().context("decode crates.io JSON")?;
    let crate_obj = json.get("crate").context("missing crate object")?;
    let max_version = crate_obj
        .get("max_version")
        .or_else(|| crate_obj.get("newest_version"))
        .and_then(|v| v.as_str())
        .context("missing max_version")?
        .to_string();

    let version = if let Some(req) = version_req {
        if req == "*" || req == "latest" {
            select_best_stable(&json).unwrap_or(max_version.clone())
        } else if let Ok(wanted) =
            semver::Version::parse(req.trim_start_matches('=').trim_start_matches('v'))
        {
            select_version(&json, &wanted.to_string())
                .ok_or_else(|| anyhow::anyhow!("crate `{name}` has no version {wanted}"))?
        } else {
            select_req(&json, req)
                .ok_or_else(|| anyhow::anyhow!("crate `{name}` has no version matching `{req}`"))?
        }
    } else {
        select_best_stable(&json).unwrap_or(max_version)
    };

    let checksum = find_checksum(&json, &version);

    Ok(ResolvedPackage {
        name: name.to_string(),
        ecosystem: "cargo".into(),
        version: version.clone(),
        source: "registry+https://github.com/rust-lang/crates.io-index".into(),
        checksum,
        git: None,
        path: None,
        url: None,
        features: None,
        default_features: None,
    })
}

fn select_version(json: &serde_json::Value, exact: &str) -> Option<String> {
    let versions = json.get("versions")?.as_array()?;
    for v in versions {
        if v.get("yanked").and_then(|y| y.as_bool()).unwrap_or(false) {
            continue;
        }
        if v.get("num")?.as_str()? == exact {
            return Some(exact.to_string());
        }
    }
    None
}

fn select_best_stable(json: &serde_json::Value) -> Option<String> {
    select_req(json, "*")
}

fn select_req(json: &serde_json::Value, req: &str) -> Option<String> {
    let req = semver::VersionReq::parse(req).ok()?;
    let versions = json.get("versions")?.as_array()?;
    let mut best: Option<semver::Version> = None;
    for v in versions {
        if v.get("yanked").and_then(|y| y.as_bool()).unwrap_or(false) {
            continue;
        }
        let num = v.get("num")?.as_str()?;
        let ver = semver::Version::parse(num).ok()?;
        if req.matches(&ver) && best.as_ref().is_none_or(|b| ver > *b) {
            best = Some(ver);
        }
    }
    best.map(|v| v.to_string())
}

fn find_checksum(json: &serde_json::Value, version: &str) -> Option<String> {
    let versions = json.get("versions")?.as_array()?;
    for v in versions {
        if v.get("num")?.as_str()? == version {
            return v
                .get("checksum")
                .and_then(|c| c.as_str())
                .map(str::to_string);
        }
    }
    None
}

pub fn search(query: &str, limit: usize) -> Result<Vec<(String, String, String)>> {
    let limit = limit.clamp(1, 100);
    let url = format!(
        "https://crates.io/api/v1/crates?q={}&per_page={}",
        urlencoding_lite(query),
        limit
    );
    let resp = agent()
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let json: serde_json::Value = resp.into_json()?;
    let mut out = Vec::new();
    if let Some(arr) = json.get("crates").and_then(|c| c.as_array()) {
        for c in arr {
            let name = c
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let max = c
                .get("max_version")
                .or_else(|| c.get("newest_version"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let desc = c
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            out.push((name, max, desc));
        }
    }
    Ok(out)
}

pub fn latest_version(name: &str) -> Result<String> {
    if !is_valid_crate_name(name) {
        bail!("invalid crate name `{name}`");
    }
    let r = fetch_crates_io(name, None)?;
    Ok(r.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_version_skips_yanked() {
        let json = serde_json::json!({
            "versions": [
                {"num": "1.0.0", "yanked": true},
                {"num": "0.9.0", "yanked": false}
            ]
        });
        assert_eq!(select_version(&json, "1.0.0"), None);
        assert_eq!(select_version(&json, "0.9.0").as_deref(), Some("0.9.0"));
    }

    #[test]
    fn select_req_picks_highest_non_yanked() {
        let json = serde_json::json!({
            "versions": [
                {"num": "2.0.0", "yanked": true},
                {"num": "1.2.0", "yanked": false},
                {"num": "1.0.0", "yanked": false}
            ]
        });
        assert_eq!(select_req(&json, "^1").as_deref(), Some("1.2.0"));
        assert_eq!(select_req(&json, "*").as_deref(), Some("1.2.0"));
    }

    #[test]
    fn crate_name_validation() {
        assert!(is_valid_crate_name("sha2"));
        assert!(!is_valid_crate_name("../x"));
        assert!(crate::util::paths::is_valid_crate_version("0.10.9"));
    }
}
