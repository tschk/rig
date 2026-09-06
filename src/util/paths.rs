use std::path::{Path, PathBuf};

pub fn join_root(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}
