//! Minimal C header scanner for simple exported function declarations.
//!
//! Intentional limits: prototypes without `{` bodies; no varargs; no macros;
//! `__attribute__(...)` / storage-class noise is stripped when present.

use crate::expose::api_scan::{ExportFn, ExportKind, FfiType, Param, TypeAdapt};
use std::path::{Path, PathBuf};

/// Scan header text for `ret name(args);` style prototypes.
pub fn scan_header_text(text: &str) -> Vec<ExportFn> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let stripped = strip_c_comments(text);
    let mut buf = String::new();
    for raw_line in stripped.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if buf.is_empty() && line.starts_with('#') {
            continue;
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(line);
        if buf.contains(';') {
            let stmt = std::mem::take(&mut buf);
            for part in stmt.split(';') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Some(export) = parse_prototype_line(part)
                    && seen.insert(export.export_name.clone())
                {
                    out.push(export);
                }
            }
        }
    }
    out
}

pub fn scan_headers(paths: &[PathBuf]) -> Vec<ExportFn> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for p in paths {
        let Ok(text) = std::fs::read_to_string(p) else {
            continue;
        };
        for e in scan_header_text(&text) {
            if seen.insert(e.export_name.clone()) {
                out.push(e);
            }
        }
    }
    out
}

fn strip_c_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(' ');
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn parse_prototype_line(line: &str) -> Option<ExportFn> {
    let mut owned = line.trim().trim_end_matches(';').trim().to_string();
    if owned.contains('{') || owned.contains('=') || owned.contains("typedef") {
        return None;
    }
    for prefix in [
        "extern ",
        "static ",
        "inline ",
        "__inline ",
        "__inline__ ",
        "RIG_API ",
        "API ",
    ] {
        if let Some(rest) = owned.strip_prefix(prefix) {
            owned = rest.trim().to_string();
        }
    }
    while let Some(start) = owned.find("__attribute__") {
        if let Some(rel) = find_matching_paren(&owned[start..]) {
            let abs = start + rel + 1;
            owned = format!("{}{}", &owned[..start], &owned[abs..]);
            owned = owned.split_whitespace().collect::<Vec<_>>().join(" ");
        } else {
            break;
        }
    }
    while let Some(start) = owned.find("__declspec") {
        if let Some(rel) = find_matching_paren(&owned[start..]) {
            let abs = start + rel + 1;
            owned = format!("{}{}", &owned[..start], &owned[abs..]);
            owned = owned.split_whitespace().collect::<Vec<_>>().join(" ");
        } else {
            break;
        }
    }

    let open = owned.rfind('(')?;
    let close = matching_close(&owned, open)?;
    if !owned[close + 1..].trim().is_empty() {
        return None;
    }
    let head = owned[..open].trim();
    let args_raw = owned[open + 1..close].trim();
    let (ret_raw, name) = split_ret_name(head)?;
    if matches!(name, "if" | "for" | "while" | "switch" | "return") {
        return None;
    }
    let ret = parse_c_type(ret_raw)?;
    let params = parse_c_params(args_raw)?;
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

fn find_matching_paren(s: &str) -> Option<usize> {
    let start = s.find('(')?;
    matching_close(s, start)
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

fn split_ret_name(head: &str) -> Option<(&str, &str)> {
    let head = head.trim();
    let bytes = head.as_bytes();
    let mut i = bytes.len();
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    if i == bytes.len() || i == 0 {
        return None;
    }
    let name = head[i..].trim();
    let ret = head[..i].trim();
    let first = name.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') || ret.is_empty() {
        return None;
    }
    Some((ret, name))
}

fn parse_c_params(args_raw: &str) -> Option<Vec<Param>> {
    let args_raw = args_raw.trim();
    if args_raw.is_empty() || args_raw == "void" {
        return Some(vec![]);
    }
    if args_raw.contains("...") {
        return None;
    }
    let mut out = Vec::new();
    for (idx, part) in split_top_level(args_raw, ',').into_iter().enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (ty_raw, name) = if let Some((ty, n)) = split_ret_name(part) {
            (ty.to_string(), n.to_string())
        } else {
            (part.to_string(), format!("arg{idx}"))
        };
        let ty = parse_c_type(&ty_raw)?;
        let name = match name.as_str() {
            "type" | "in" | "out" | "string" | "mod" | "fn" | "let" | "pub" => format!("{name}_"),
            _ => name,
        };
        out.push(Param {
            name,
            ty,
            adapt: TypeAdapt::Identity,
        });
    }
    Some(out)
}

fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                cur.push(ch);
            }
            ')' | ']' => {
                depth -= 1;
                cur.push(ch);
            }
            c if c == sep && depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

fn parse_c_type(raw: &str) -> Option<FfiType> {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = compact.replace(" *", "*").replace("* ", "*");

    // Normalize common pointer spellings first.
    match compact.as_str() {
        "const char*" | "char const*" => return Some(FfiType::ConstCChar),
        "char*" => return Some(FfiType::MutCChar),
        "const void*" | "void const*" => return Some(FfiType::ConstVoid),
        "void*" => return Some(FfiType::MutVoid),
        _ => {}
    }
    if let Some(rest) = compact.strip_suffix('*') {
        let rest = rest.trim();
        let (inner, is_const) = if let Some(r) = rest.strip_prefix("const ") {
            (r.trim(), true)
        } else if let Some(r) = rest.strip_suffix(" const") {
            (r.trim(), true)
        } else {
            (rest, false)
        };
        // multi-star: recurse
        let inner_ty = if inner.ends_with('*') {
            parse_c_type(inner)?
        } else {
            parse_c_base(inner)?
        };
        return Some(if is_const {
            FfiType::ConstPtr(Box::new(inner_ty))
        } else {
            FfiType::MutPtr(Box::new(inner_ty))
        });
    }
    parse_c_base(&compact)
}

fn parse_c_base(base: &str) -> Option<FfiType> {
    let base = base.trim();
    Some(match base {
        "void" => FfiType::Void,
        "bool" | "_Bool" => FfiType::Bool,
        "char" | "signed char" => FfiType::I8,
        "unsigned char" | "uint8_t" => FfiType::U8,
        "short" | "short int" | "signed short" | "signed short int" | "int16_t" => FfiType::I16,
        "unsigned short" | "unsigned short int" | "uint16_t" => FfiType::U16,
        "int" | "signed" | "signed int" | "int32_t" => FfiType::I32,
        "unsigned" | "unsigned int" | "uint32_t" => FfiType::U32,
        "long long" | "long long int" | "signed long long" | "int64_t" => FfiType::I64,
        "unsigned long long" | "unsigned long long int" | "uint64_t" => FfiType::U64,
        "size_t" => FfiType::Usize,
        "ptrdiff_t" | "ssize_t" => FfiType::Isize,
        "float" => FfiType::F32,
        "double" => FfiType::F64,
        // pragmatic LP64 default for binders
        "long" | "long int" | "signed long" => FfiType::Isize,
        "unsigned long" => FfiType::Usize,
        _ => return None,
    })
}

#[allow(dead_code)]
pub fn headers_under(root: &Path) -> Vec<PathBuf> {
    let mut headers = Vec::new();
    let walker = walkdir::WalkDir::new(root).max_depth(2);
    for ent in walker.into_iter().filter_map(|e| e.ok()) {
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) == Some("h") {
            headers.push(p.to_path_buf());
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_simple_prototypes() {
        let hdr = r#"
#pragma once
/* comment */
int flatlib_add(int a, int b);
const char *flatlib_name(void);
void flatlib_touch(uint8_t *data, size_t len);
int flatlib_scale(
    int value,
    int factor
);
"#;
        let exports = scan_header_text(hdr);
        let names: Vec<_> = exports.iter().map(|e| e.export_name.as_str()).collect();
        assert!(names.contains(&"flatlib_add"), "{names:?}");
        assert!(names.contains(&"flatlib_name"), "{names:?}");
        assert!(names.contains(&"flatlib_touch"), "{names:?}");
        assert!(names.contains(&"flatlib_scale"), "{names:?}");
        let add = exports
            .iter()
            .find(|e| e.export_name == "flatlib_add")
            .unwrap();
        assert_eq!(add.ret, FfiType::I32);
        assert_eq!(add.params.len(), 2);
        let name = exports
            .iter()
            .find(|e| e.export_name == "flatlib_name")
            .unwrap();
        assert_eq!(name.ret, FfiType::ConstCChar);
    }
}
