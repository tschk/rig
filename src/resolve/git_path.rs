//! Shared path/git pin helpers for ecosystems without (or beyond) a registry.

use super::{PackageSpec, ResolvedPackage};
use anyhow::{Result, bail};

/// If `spec` carries path or git, build a ResolvedPackage for `ecosystem`.
pub fn resolve_path_or_git(
    ecosystem: &str,
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Option<ResolvedPackage> {
    if let Some(git) = &spec.git {
        return Some(ResolvedPackage {
            name: spec.name.clone(),
            ecosystem: ecosystem.into(),
            version: spec.version_req.clone().unwrap_or_else(|| "git".into()),
            source: format!("git+{git}"),
            checksum: None,
            git: Some(git.clone()),
            path: None,
            url: None,
            features,
            default_features: if no_default { Some(false) } else { None },
        });
    }
    if let Some(path) = &spec.path {
        return Some(ResolvedPackage {
            name: spec.name.clone(),
            ecosystem: ecosystem.into(),
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
    None
}

/// Ecosystems that only support path/git pins (no registry client).
pub fn resolve_path_git_only(
    ecosystem: &str,
    spec: &PackageSpec,
    features: Option<Vec<String>>,
    no_default: bool,
) -> Result<ResolvedPackage> {
    if let Some(pkg) = resolve_path_or_git(ecosystem, spec, features, no_default) {
        return Ok(pkg);
    }
    bail!(
        "{ecosystem} has no central package registry — use path:… or git+…\n\
         example: rig add --{ecosystem} path:./vendor/dep\n\
         example: rig add --{ecosystem} git+https://github.com/org/dep"
    )
}

pub fn search_unsupported(ecosystem: &str) -> String {
    format!(
        "search ({ecosystem}): no central registry — pin with path:… or git+… via `rig add --{ecosystem}`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_pin() {
        let spec = PackageSpec {
            name: "dep".into(),
            version_req: None,
            git: None,
            path: Some("./x".into()),
            url: None,
        };
        let r = resolve_path_or_git("c", &spec, None, false).unwrap();
        assert_eq!(r.version, "path");
        assert_eq!(r.source, "path:./x");
    }

    #[test]
    fn bare_name_errors() {
        let spec = PackageSpec {
            name: "foo".into(),
            version_req: None,
            git: None,
            path: None,
            url: None,
        };
        assert!(resolve_path_git_only("odin", &spec, None, false).is_err());
    }
}
