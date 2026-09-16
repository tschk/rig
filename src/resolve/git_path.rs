//! Shared path/git pin helpers for ecosystems without (or beyond) a registry.

use super::{PackageSpec, ResolvedPackage};
use anyhow::{Result, bail};

/// Reject git URLs that git would treat as options or unexpected schemes.
pub fn validate_git_url(url: &str) -> Result<()> {
    let url = url.trim();
    if url.is_empty()
        || url.starts_with('-')
        || url.contains('\n')
        || url.contains('\r')
        || url.contains('\0')
        || url.contains(' ')
    {
        bail!("invalid git URL");
    }
    let ok = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("ssh://")
        || url.starts_with("git://")
        || url.starts_with("git@");
    if !ok {
        bail!(
            "git URL must be https://, http://, ssh://, git://, or scp-like git@host:path (got `{url}`)"
        );
    }
    Ok(())
}

/// Git rev / tag / branch: no whitespace, no leading `-`, no control chars.
pub fn validate_git_rev(rev: &str) -> Result<()> {
    let rev = rev.trim();
    if rev.is_empty()
        || rev.starts_with('-')
        || rev.contains('\n')
        || rev.contains('\r')
        || rev.contains('\0')
        || rev.contains(' ')
        || rev.contains("..")
    {
        bail!("invalid git rev `{rev}`");
    }
    if !rev
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '~' | '^'))
    {
        bail!("invalid git rev `{rev}`");
    }
    Ok(())
}

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
            version: spec
                .rev
                .clone()
                .or_else(|| spec.version_req.clone())
                .unwrap_or_else(|| "git".into()),
            source: format!("git+{git}"),
            checksum: None,
            git: Some(git.clone()),
            rev: spec.rev.clone(),
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
            rev: None,
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
            rev: None,
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
            rev: None,
            path: None,
            url: None,
        };
        assert!(resolve_path_git_only("odin", &spec, None, false).is_err());
    }

    #[test]
    fn git_url_rejects_option_injection() {
        assert!(validate_git_url("-uorigin").is_err());
        assert!(validate_git_url("file:///etc/passwd").is_err());
        assert!(validate_git_url("https://github.com/a/b.git").is_ok());
        assert!(validate_git_url("git@github.com:a/b.git").is_ok());
    }

    #[test]
    fn git_rev_rejects_option_injection() {
        assert!(validate_git_rev("-uorigin").is_err());
        assert!(validate_git_rev("abc..def").is_err());
        assert!(validate_git_rev("deadbeef").is_ok());
        assert!(validate_git_rev("v1.2.3").is_ok());
        assert!(validate_git_rev("refs/heads/main").is_ok());
    }
}
