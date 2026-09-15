use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};

pub fn join_root(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

/// Lexically normalize `.` / `..` without touching the filesystem.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

/// True when `candidate` stays under `root` after lexical `..` resolution.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let root_n = normalize_lexically(root);
    let cand_n = normalize_lexically(candidate);
    cand_n.starts_with(&root_n)
}

/// Join a relative path onto `root`. Absolute / rooted paths pass through
/// unchanged (`is_absolute` is false on Windows for `/foo`, so also check
/// `has_root`).
/// Relative paths that would escape `root` via `..` are rejected.
pub fn confine_relative(root: &Path, rel: &Path) -> Result<PathBuf> {
    if rel.is_absolute() || rel.has_root() {
        return Ok(rel.to_path_buf());
    }
    let joined = root.join(rel);
    if !is_within(root, &joined) {
        bail!(
            "path `{}` escapes project root `{}`",
            rel.display(),
            root.display()
        );
    }
    Ok(joined)
}

/// crates.io crate names: ASCII alnum / `_` / `-`, start with a letter, ≤64 chars.
pub fn is_valid_crate_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > 64 {
        return false;
    }
    if !b[0].is_ascii_alphabetic() {
        return false;
    }
    b.iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

/// Version string safe to use in URLs and cache paths (no separators / traversal).
pub fn is_valid_crate_version(version: &str) -> bool {
    let b = version.as_bytes();
    if b.is_empty() || b.len() > 64 {
        return false;
    }
    b.iter()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn relative_stays_inside() {
        let root = Path::new("/proj");
        let p = confine_relative(root, Path::new("vendor/lib")).unwrap();
        assert_eq!(p, Path::new("/proj/vendor/lib"));
    }

    #[test]
    fn parent_dir_escape_rejected() {
        let root = Path::new("/proj");
        assert!(confine_relative(root, Path::new("../etc/passwd")).is_err());
        assert!(confine_relative(root, Path::new("foo/../../etc")).is_err());
    }

    #[test]
    fn absolute_passthrough() {
        let p = confine_relative(Path::new("/proj"), Path::new("/opt/lib")).unwrap();
        assert_eq!(p, Path::new("/opt/lib"));
    }

    #[test]
    fn crate_name_rules() {
        assert!(is_valid_crate_name("sha2"));
        assert!(is_valid_crate_name("serde_json"));
        assert!(is_valid_crate_name("md-5"));
        assert!(!is_valid_crate_name(""));
        assert!(!is_valid_crate_name("../sha2"));
        assert!(!is_valid_crate_name("sha2/../../x"));
        assert!(!is_valid_crate_name("sha2?x=1"));
        assert!(!is_valid_crate_name("1sha2"));
    }

    #[test]
    fn crate_version_rules() {
        assert!(is_valid_crate_version("0.10.9"));
        assert!(is_valid_crate_version("1.0.0-beta.1"));
        assert!(!is_valid_crate_version("../1.0"));
        assert!(!is_valid_crate_version("1.0/../../x"));
    }
}
