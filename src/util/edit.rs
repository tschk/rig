use anyhow::{Context, Result};
use std::path::Path;

/// Idempotently ensure `mod rig_bindings;` exists in lib.rs or main.rs.
pub fn ensure_rust_mod_decl(root: &Path) -> Result<()> {
    for candidate in ["src/lib.rs", "src/main.rs"] {
        let path = root.join(candidate);
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        if text.contains("mod rig_bindings") || text.contains("pub mod rig_bindings") {
            return Ok(());
        }
        let decl = "mod rig_bindings;\n";
        let new_text = if text.starts_with("//!") {
            // after module docs
            let mut lines = text.lines().peekable();
            let mut out = String::new();
            while let Some(line) = lines.peek() {
                if line.starts_with("//!") || line.trim().is_empty() {
                    out.push_str(lines.next().unwrap());
                    out.push('\n');
                } else {
                    break;
                }
            }
            out.push_str(decl);
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
            out
        } else {
            format!("{decl}{text}")
        };
        std::fs::write(&path, new_text).with_context(|| format!("update {}", path.display()))?;
        return Ok(());
    }
    Ok(())
}

/// Add or update a dependency in the host Cargo.toml using toml_edit.
pub fn cargo_add_dep(
    cargo_toml: &Path,
    name: &str,
    version: &str,
    git: Option<&str>,
    features: Option<&[String]>,
    default_features: Option<bool>,
) -> Result<()> {
    let text = std::fs::read_to_string(cargo_toml)
        .with_context(|| format!("read {}", cargo_toml.display()))?;
    let mut doc: toml_edit::DocumentMut = text.parse().context("parse Cargo.toml")?;
    let deps = doc
        .entry("dependencies")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("dependencies table")?;

    if let Some(git) = git {
        let mut table = toml_edit::InlineTable::new();
        table.insert("git", git.into());
        if version != "git" && version != "*" {
            // optional: don't set version with git
        }
        if let Some(feats) = features {
            let mut arr = toml_edit::Array::new();
            for f in feats {
                arr.push(f.as_str());
            }
            table.insert("features", arr.into());
        }
        if default_features == Some(false) {
            table.insert("default-features", false.into());
        }
        deps[name] = toml_edit::Item::Value(toml_edit::Value::InlineTable(table));
    } else if let Some(feats) = features {
        let mut table = toml_edit::InlineTable::new();
        table.insert("version", version.into());
        let mut arr = toml_edit::Array::new();
        for f in feats {
            arr.push(f.as_str());
        }
        table.insert("features", arr.into());
        if default_features == Some(false) {
            table.insert("default-features", false.into());
        }
        deps[name] = toml_edit::Item::Value(toml_edit::Value::InlineTable(table));
    } else if default_features == Some(false) {
        let mut table = toml_edit::InlineTable::new();
        table.insert("version", version.into());
        table.insert("default-features", false.into());
        deps[name] = toml_edit::Item::Value(toml_edit::Value::InlineTable(table));
    } else {
        deps[name] = toml_edit::value(version);
    }

    std::fs::write(cargo_toml, doc.to_string())
        .with_context(|| format!("write {}", cargo_toml.display()))?;
    Ok(())
}

pub fn cargo_remove_dep(cargo_toml: &Path, name: &str) -> Result<bool> {
    if !cargo_toml.is_file() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(cargo_toml)?;
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let Some(deps) = doc.get_mut("dependencies").and_then(|i| i.as_table_mut()) else {
        return Ok(false);
    };
    let removed = deps.remove(name).is_some();
    if removed {
        std::fs::write(cargo_toml, doc.to_string())?;
    }
    Ok(removed)
}

/// Idempotent build.zig patch markers.
pub fn patch_build_zig_link(build_zig: &Path, pkg: &str, lib_hint: &str) -> Result<()> {
    const BEGIN: &str = "// rig-expose-begin";
    const END: &str = "// rig-expose-end";
    let mut text = if build_zig.is_file() {
        std::fs::read_to_string(build_zig)?
    } else {
        String::new()
    };
    let block = format!(
        "{BEGIN}\n// rig: link {pkg} native lib (path hint: {lib_hint})\n// Add: exe.addLibraryPath / exe.linkSystemLibrary as appropriate for your Zig version.\n{END}\n"
    );
    if text.contains(BEGIN) {
        // replace region
        if let (Some(s), Some(e)) = (text.find(BEGIN), text.find(END)) {
            let end = e + END.len();
            text.replace_range(s..end, block.trim_end());
            text.push('\n');
        }
    } else {
        text.push('\n');
        text.push_str(&block);
    }
    std::fs::write(build_zig, text)?;
    Ok(())
}
