//! Shared HTTP helpers for registry clients.

use ureq::Agent;

pub fn agent() -> Agent {
    ureq::AgentBuilder::new()
        .user_agent("rig/0.1 (+https://github.com/tschk/rig)")
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build()
}

pub fn urlencoding_lite(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Pick the highest semver from `candidates` that matches `req` (if any).
/// Prefers non-prerelease unless `req` explicitly needs one / none match.
pub fn pick_semver(candidates: &[String], req: Option<&str>) -> Option<String> {
    let parsed: Vec<(String, semver::Version)> = candidates
        .iter()
        .filter_map(|s| {
            let cleaned = s.trim().trim_start_matches('v');
            semver::Version::parse(cleaned).ok().map(|v| (s.clone(), v))
        })
        .collect();
    if parsed.is_empty() {
        return candidates.last().cloned();
    }

    if let Some(req) = req {
        if req == "*" || req == "latest" {
            // fall through to best
        } else if let Ok(exact) =
            semver::Version::parse(req.trim_start_matches('=').trim_start_matches('v'))
        {
            if let Some((s, _)) = parsed.iter().find(|(_, v)| *v == exact) {
                return Some(s.clone());
            }
        } else if let Ok(vr) = semver::VersionReq::parse(req) {
            let mut best: Option<&(String, semver::Version)> = None;
            for cand in &parsed {
                if vr.matches(&cand.1) && best.map(|b| cand.1 > b.1).unwrap_or(true) {
                    best = Some(cand);
                }
            }
            if let Some((s, _)) = best {
                return Some(s.clone());
            }
        }
    }

    // Prefer highest non-prerelease; else highest overall
    let mut best_stable: Option<&(String, semver::Version)> = None;
    let mut best_any: Option<&(String, semver::Version)> = None;
    for cand in &parsed {
        if best_any.map(|b| cand.1 > b.1).unwrap_or(true) {
            best_any = Some(cand);
        }
        if cand.1.pre.is_empty() && best_stable.map(|b| cand.1 > b.1).unwrap_or(true) {
            best_stable = Some(cand);
        }
    }
    best_stable.or(best_any).map(|(s, _)| s.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_prefers_stable() {
        let v = vec![
            "1.0.0-rc.1".into(),
            "0.9.0".into(),
            "1.0.0".into(),
            "1.1.0-beta.1".into(),
        ];
        assert_eq!(pick_semver(&v, None).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn pick_respects_req() {
        let v = vec!["1.0.0".into(), "1.2.0".into(), "2.0.0".into()];
        assert_eq!(pick_semver(&v, Some("^1")).as_deref(), Some("1.2.0"));
    }
}
