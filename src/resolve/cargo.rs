use super::http::{agent, urlencoding_lite};
use super::{PackageSpec, ResolvedPackage};
use anyhow::{Context, Result};

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
    // Special-case rx4/rotary → prefer crates.io rx4, note rotary git
    let (query_name, git_fallback) = if name == "rotary" || name == "rx4" {
        (
            "rx4".to_string(),
            Some("https://github.com/tschk/rotary".to_string()),
        )
    } else {
        (name.clone(), None)
    };

    match fetch_crates_io(&query_name, spec.version_req.as_deref()) {
        Ok(mut resolved) => {
            resolved.features = features;
            resolved.default_features = if no_default { Some(false) } else { None };
            // Keep registry resolution; git_fallback is only for lookup failure.
            let _ = git_fallback;
            Ok(resolved)
        }
        Err(err) => {
            if let Some(git) = git_fallback {
                eprintln!("warn: crates.io lookup failed ({err:#}); pinning git {git}");
                Ok(ResolvedPackage {
                    name: query_name,
                    ecosystem: "cargo".into(),
                    version: spec.version_req.clone().unwrap_or_else(|| "*".into()),
                    source: format!("git+{git}"),
                    checksum: None,
                    git: Some(git),
                    path: None,
                    url: None,
                    features,
                    default_features: if no_default { Some(false) } else { None },
                })
            } else {
                Err(err)
            }
        }
    }
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
            max_version.clone()
        } else if let Ok(wanted) =
            semver::Version::parse(req.trim_start_matches('=').trim_start_matches('v'))
        {
            // exact or find matching
            select_version(&json, &wanted.to_string()).unwrap_or(wanted.to_string())
        } else {
            // treat as requirement string — pick max_version if it matches loosely
            select_req(&json, req).unwrap_or(max_version.clone())
        }
    } else {
        max_version
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
        if v.get("num")?.as_str()? == exact {
            return Some(exact.to_string());
        }
    }
    None
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
    let r = fetch_crates_io(name, None)?;
    Ok(r.version)
}
