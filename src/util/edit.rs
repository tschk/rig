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

/// Idempotent build.zig patch: link the rig-built cdylib (Zig 0.14+ / 0.16 API).
///
/// Inserts **per-package** markers and wires `root_module.addLibraryPath` +
/// `linkSystemLibrary` + `addRPath` + `addIncludePath` for the façade under
/// `lib_hint`. Multiple `rig add` / `rig sync` calls accumulate without clobbering.
pub fn patch_build_zig_link(
    build_zig: &Path,
    pkg: &str,
    lib_hint: &str,
    lib_name: &str,
) -> Result<()> {
    let begin = format!("// rig-expose-begin:{pkg}");
    let end = format!("// rig-expose-end:{pkg}");
    let mut text = if build_zig.is_file() {
        std::fs::read_to_string(build_zig)?
    } else {
        String::new()
    };

    // Migrate legacy single unscoped region once (first multi-pkg sync).
    const LEGACY_BEGIN: &str = "// rig-expose-begin";
    const LEGACY_END: &str = "// rig-expose-end";
    let has_scoped = text
        .lines()
        .any(|l| l.trim().starts_with("// rig-expose-begin:"));
    if !has_scoped {
        let legacy = text.lines().any(|l| l.trim() == LEGACY_BEGIN)
            && text.lines().any(|l| l.trim() == LEGACY_END);
        if legacy {
            // Drop the entire legacy block; per-pkg blocks replace it.
            if let (Some(s), Some(e)) = (text.find(LEGACY_BEGIN), text.find(LEGACY_END)) {
                let s = text[..s].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end_idx = e + LEGACY_END.len();
                let end_idx = if text[end_idx..].starts_with('\n') {
                    end_idx + 1
                } else {
                    end_idx
                };
                text.replace_range(s..end_idx, "");
            }
        }
    }

    let block = format!(
        "    {begin}\n    // rig: link `{pkg}` façade `{lib_name}` from {lib_hint}\n    exe.root_module.addLibraryPath(b.path(\"{lib_hint}\"));\n    exe.root_module.addRPath(b.path(\"{lib_hint}\"));\n    exe.root_module.addIncludePath(b.path(\"{lib_hint}\"));\n    exe.root_module.linkSystemLibrary(\"{lib_name}\", .{{}});\n    {end}\n"
    );

    let has_region =
        text.lines().any(|l| l.trim() == begin) && text.lines().any(|l| l.trim() == end);
    if has_region {
        if let (Some(s), Some(e)) = (text.find(&begin), text.find(&end)) {
            let s = text[..s].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let end_idx = e + end.len();
            text.replace_range(s..end_idx, block.trim_end());
            if !text[s..].starts_with('\n') && s > 0 {
                // keep surrounding newlines tidy
            }
            if !text[end_idx.min(text.len())..].starts_with('\n') {
                // replace_range already removed old end; ensure newline after block
            }
            // Ensure a trailing newline after the replaced block
            let after = s + block.trim_end().len();
            if after >= text.len() || !text[after..].starts_with('\n') {
                text.insert(after.min(text.len()), '\n');
            }
        }
    } else if let Some(idx) = text.find("b.installArtifact(exe);") {
        text.insert_str(idx, &format!("\n{block}\n    "));
    } else {
        text.push('\n');
        text.push_str(&block);
    }
    std::fs::write(build_zig, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn patch_build_zig_accumulates_packages() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("build.zig");
        fs::write(
            &build,
            r#"const std = @import("std");
pub fn build(b: *std.Build) void {
    const exe = b.addExecutable(.{ .name = "demo", .root_module = b.createModule(.{
        .root_source_file = b.path("src/main.zig"),
        .target = b.graph.host,
        .optimize = .Debug,
    })});
    b.installArtifact(exe);
}
"#,
        )
        .unwrap();
        patch_build_zig_link(&build, "sha2", "target/rig/sha2", "sha2_ffi").unwrap();
        patch_build_zig_link(&build, "rx4", "target/rig/rx4", "rx4_ffi").unwrap();
        let text = fs::read_to_string(&build).unwrap();
        assert!(text.contains("// rig-expose-begin:sha2"));
        assert!(text.contains("// rig-expose-begin:rx4"));
        assert!(text.contains("linkSystemLibrary(\"sha2_ffi\""));
        assert!(text.contains("linkSystemLibrary(\"rx4_ffi\""));
        // Idempotent update
        patch_build_zig_link(&build, "sha2", "target/rig/sha2", "sha2_ffi").unwrap();
        let text2 = fs::read_to_string(&build).unwrap();
        assert_eq!(
            text2.matches("// rig-expose-begin:sha2").count(),
            1,
            "sha2 block should remain unique"
        );
    }
}
