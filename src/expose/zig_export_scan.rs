//! Minimal Zig `export fn` scanner for path/git Zig shared libs.

use crate::expose::api_scan::{ExportFn, ExportKind, FfiType, Param, TypeAdapt};
use std::path::{Path, PathBuf};

pub fn scan_zig_sources(root: &Path) -> Vec<ExportFn> {
    let mut outs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in zig_files(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for e in scan_zig_text(&text) {
            if seen.insert(e.export_name.clone()) {
                outs.push(e);
            }
        }
    }
    outs
}

fn zig_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = walkdir::WalkDir::new(root).max_depth(3);
    for ent in walker.into_iter().filter_map(|e| e.ok()) {
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) == Some("zig") {
            files.push(p.to_path_buf());
        }
    }
    files
}

pub fn scan_zig_text(text: &str) -> Vec<ExportFn> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        // export fn name(...) T {
        // export fn name(...) T;
        if !line.starts_with("export fn ") {
            continue;
        }
        if let Some(e) = parse_export_fn(line)
            && seen.insert(e.export_name.clone())
        {
            out.push(e);
        }
    }
    out
}

fn parse_export_fn(line: &str) -> Option<ExportFn> {
    let rest = line.strip_prefix("export fn ")?.trim();
    let open = rest.find('(')?;
    let name = rest[..open].trim();
    if name.is_empty() {
        return None;
    }
    let close = matching_close(rest, open)?;
    let args_raw = rest[open + 1..close].trim();
    let after = rest[close + 1..].trim();
    let ret_raw = after
        .split('{')
        .next()
        .unwrap_or(after)
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let ret = if ret_raw.is_empty() || ret_raw == "void" {
        FfiType::Void
    } else {
        parse_zig_type(ret_raw)?
    };
    let params = parse_zig_params(args_raw)?;
    Some(ExportFn {
        export_name: name.to_string(),
        rust_callee: None,
        params,
        ret,
        ret_adapt: TypeAdapt::Identity,
        kind: ExportKind::UpstreamExternC,
        is_unsafe: false,
    })
}

fn matching_close(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_zig_params(args_raw: &str) -> Option<Vec<Param>> {
    let args_raw = args_raw.trim();
    if args_raw.is_empty() {
        return Some(vec![]);
    }
    let mut out = Vec::new();
    for (idx, part) in args_raw.split(',').enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, ty_raw) = if let Some((n, t)) = part.split_once(':') {
            (n.trim().to_string(), t.trim())
        } else {
            (format!("arg{idx}"), part)
        };
        let ty = parse_zig_type(ty_raw)?;
        out.push(Param {
            name,
            ty,
            adapt: TypeAdapt::Identity,
        });
    }
    Some(out)
}

fn parse_zig_type(raw: &str) -> Option<FfiType> {
    let t = raw.trim();
    Some(match t {
        "void" => FfiType::Void,
        "bool" => FfiType::Bool,
        "u8" => FfiType::U8,
        "u16" => FfiType::U16,
        "u32" => FfiType::U32,
        "u64" => FfiType::U64,
        "usize" => FfiType::Usize,
        "i8" => FfiType::I8,
        "i16" => FfiType::I16,
        "i32" | "c_int" => FfiType::I32,
        "i64" => FfiType::I64,
        "isize" => FfiType::Isize,
        "f32" => FfiType::F32,
        "f64" => FfiType::F64,
        "[*:0]const u8" | "[*:0]u8" | "[*]const u8" => FfiType::ConstCChar,
        "[*]u8" => FfiType::MutCChar,
        "?*anyopaque" | "*anyopaque" => FfiType::MutVoid,
        "?*const anyopaque" | "*const anyopaque" => FfiType::ConstVoid,
        other if other.starts_with("[*]const ") => {
            let inner = parse_zig_type(other.trim_start_matches("[*]const ").trim())?;
            FfiType::ConstPtr(Box::new(inner))
        }
        other if other.starts_with("[*]") => {
            let inner = parse_zig_type(other.trim_start_matches("[*]").trim())?;
            FfiType::MutPtr(Box::new(inner))
        }
        other if other.starts_with('*') => {
            let inner = parse_zig_type(other.trim_start_matches('*').trim())?;
            FfiType::MutPtr(Box::new(inner))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_export_fn() {
        let src = r#"
export fn zmath_add(a: i32, b: i32) i32 {
    return a + b;
}
export fn zmath_name() [*:0]const u8 {
    return "zmath";
}
"#;
        let exports = scan_zig_text(src);
        let names: Vec<_> = exports.iter().map(|e| e.export_name.as_str()).collect();
        assert!(names.contains(&"zmath_add"), "{names:?}");
        assert!(names.contains(&"zmath_name"), "{names:?}");
        let add = exports
            .iter()
            .find(|e| e.export_name == "zmath_add")
            .unwrap();
        assert_eq!(add.params.len(), 2);
        assert_eq!(add.ret, FfiType::I32);
    }
}
