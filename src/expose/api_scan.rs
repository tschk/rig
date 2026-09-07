//! Public-API scan for simple extern-C-friendly Rust functions.
//!
//! Scans crate sources for:
//! 1. Existing `#[no_mangle] extern "C"` / `extern "C"` exports (re-exported as-is).
//! 2. Plain `pub fn` / `pub const fn` with only FFI-safe scalar/pointer types
//!    (wrapped as `{crate}_{fn}`).
//!
//! Honest skips: generics, traits/`impl` methods, `async`, tuples/arrays/refs,
//! `String`/`str`/`Vec`, non-`repr(C)` structs, `f16`/`f128`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Canonical FFI type used across façade + host binders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiType {
    Void,
    U8,
    U16,
    U32,
    U64,
    Usize,
    I8,
    I16,
    I32,
    I64,
    Isize,
    F32,
    F64,
    Bool,
    /// `*const c_char` / C string
    ConstCChar,
    MutCChar,
    ConstVoid,
    MutVoid,
    ConstPtr(Box<FfiType>),
    MutPtr(Box<FfiType>),
}

impl FfiType {
    pub fn rust_ty(&self) -> String {
        match self {
            Self::Void => "()".into(),
            Self::U8 => "u8".into(),
            Self::U16 => "u16".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::Usize => "usize".into(),
            Self::I8 => "i8".into(),
            Self::I16 => "i16".into(),
            Self::I32 => "i32".into(),
            Self::I64 => "i64".into(),
            Self::Isize => "isize".into(),
            Self::F32 => "f32".into(),
            Self::F64 => "f64".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar => "*const c_char".into(),
            Self::MutCChar => "*mut c_char".into(),
            Self::ConstVoid => "*const c_void".into(),
            Self::MutVoid => "*mut c_void".into(),
            Self::ConstPtr(inner) => format!("*const {}", inner.rust_ty()),
            Self::MutPtr(inner) => format!("*mut {}", inner.rust_ty()),
        }
    }

    pub fn c_ty(&self) -> String {
        match self {
            Self::Void => "void".into(),
            Self::U8 => "uint8_t".into(),
            Self::U16 => "uint16_t".into(),
            Self::U32 => "uint32_t".into(),
            Self::U64 => "uint64_t".into(),
            Self::Usize => "size_t".into(),
            Self::I8 => "int8_t".into(),
            Self::I16 => "int16_t".into(),
            Self::I32 => "int32_t".into(),
            Self::I64 => "int64_t".into(),
            Self::Isize => "ptrdiff_t".into(),
            Self::F32 => "float".into(),
            Self::F64 => "double".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar => "const char *".into(),
            Self::MutCChar => "char *".into(),
            Self::ConstVoid => "const void *".into(),
            Self::MutVoid => "void *".into(),
            Self::ConstPtr(inner) => format!("const {} *", inner.c_ty_base()),
            Self::MutPtr(inner) => format!("{} *", inner.c_ty_base()),
        }
    }

    fn c_ty_base(&self) -> String {
        match self {
            Self::ConstPtr(_)
            | Self::MutPtr(_)
            | Self::ConstCChar
            | Self::MutCChar
            | Self::ConstVoid
            | Self::MutVoid => self.c_ty(),
            other => other.c_ty(),
        }
    }

    pub fn zig_ty(&self) -> String {
        match self {
            Self::Void => "void".into(),
            Self::U8 => "u8".into(),
            Self::U16 => "u16".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::Usize => "usize".into(),
            Self::I8 => "i8".into(),
            Self::I16 => "i16".into(),
            Self::I32 => "i32".into(),
            Self::I64 => "i64".into(),
            Self::Isize => "isize".into(),
            Self::F32 => "f32".into(),
            Self::F64 => "f64".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar => "[*:0]const u8".into(),
            Self::MutCChar => "[*:0]u8".into(),
            Self::ConstVoid => "?*const anyopaque".into(),
            Self::MutVoid => "?*anyopaque".into(),
            Self::ConstPtr(inner) => format!("[*]const {}", inner.zig_ty()),
            Self::MutPtr(inner) => format!("[*]{}", inner.zig_ty()),
        }
    }

    pub fn nim_ty(&self) -> String {
        match self {
            Self::Void => "".into(),
            Self::U8 => "uint8".into(),
            Self::U16 => "uint16".into(),
            Self::U32 => "uint32".into(),
            Self::U64 => "uint64".into(),
            Self::Usize => "csize_t".into(),
            Self::I8 => "int8".into(),
            Self::I16 => "int16".into(),
            Self::I32 => "cint".into(),
            Self::I64 => "int64".into(),
            Self::Isize => "csize_t".into(),
            Self::F32 => "cfloat".into(),
            Self::F64 => "cdouble".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar | Self::MutCChar => "cstring".into(),
            Self::ConstVoid | Self::MutVoid => "pointer".into(),
            Self::ConstPtr(inner) | Self::MutPtr(inner) => format!("ptr {}", inner.nim_ty()),
        }
    }

    pub fn csharp_ty(&self) -> String {
        match self {
            Self::Void => "void".into(),
            Self::U8 => "byte".into(),
            Self::U16 => "ushort".into(),
            Self::U32 => "uint".into(),
            Self::U64 => "ulong".into(),
            Self::Usize => "UIntPtr".into(),
            Self::I8 => "sbyte".into(),
            Self::I16 => "short".into(),
            Self::I32 => "int".into(),
            Self::I64 => "long".into(),
            Self::Isize => "IntPtr".into(),
            Self::F32 => "float".into(),
            Self::F64 => "double".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar | Self::MutCChar => "string".into(),
            Self::ConstVoid | Self::MutVoid => "IntPtr".into(),
            Self::ConstPtr(inner) if matches!(inner.as_ref(), Self::U8) => "byte[]".into(),
            Self::MutPtr(inner) if matches!(inner.as_ref(), Self::U8) => "byte[]".into(),
            Self::ConstPtr(_) | Self::MutPtr(_) => "IntPtr".into(),
        }
    }

    pub fn d_ty(&self) -> String {
        match self {
            Self::Void => "void".into(),
            Self::U8 => "ubyte".into(),
            Self::U16 => "ushort".into(),
            Self::U32 => "uint".into(),
            Self::U64 => "ulong".into(),
            Self::Usize => "size_t".into(),
            Self::I8 => "byte".into(),
            Self::I16 => "short".into(),
            Self::I32 => "int".into(),
            Self::I64 => "long".into(),
            Self::Isize => "ptrdiff_t".into(),
            Self::F32 => "float".into(),
            Self::F64 => "double".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar => "const(char)*".into(),
            Self::MutCChar => "char*".into(),
            Self::ConstVoid => "const(void)*".into(),
            Self::MutVoid => "void*".into(),
            Self::ConstPtr(inner) => format!("const({})*", inner.d_ty()),
            Self::MutPtr(inner) => format!("{}*", inner.d_ty()),
        }
    }

    pub fn v_ty(&self) -> String {
        match self {
            Self::Void => "".into(),
            Self::U8 => "u8".into(),
            Self::U16 => "u16".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::Usize => "usize".into(),
            Self::I8 => "i8".into(),
            Self::I16 => "i16".into(),
            Self::I32 => "int".into(),
            Self::I64 => "i64".into(),
            Self::Isize => "isize".into(),
            Self::F32 => "f32".into(),
            Self::F64 => "f64".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar | Self::MutCChar => "&char".into(),
            Self::ConstVoid | Self::MutVoid => "voidptr".into(),
            Self::ConstPtr(inner) | Self::MutPtr(inner) => format!("&{}", inner.v_ty()),
        }
    }

    pub fn odin_ty(&self) -> String {
        match self {
            Self::Void => "".into(),
            Self::U8 => "u8".into(),
            Self::U16 => "u16".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::Usize => "uint".into(),
            Self::I8 => "i8".into(),
            Self::I16 => "i16".into(),
            Self::I32 => "i32".into(),
            Self::I64 => "i64".into(),
            Self::Isize => "int".into(),
            Self::F32 => "f32".into(),
            Self::F64 => "f64".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar => "cstring".into(),
            Self::MutCChar => "cstring".into(),
            Self::ConstVoid => "rawptr".into(),
            Self::MutVoid => "rawptr".into(),
            Self::ConstPtr(inner) => format!("^{}", inner.odin_ty()),
            Self::MutPtr(inner) => format!("^{}", inner.odin_ty()),
        }
    }

    pub fn hare_ty(&self) -> String {
        match self {
            Self::Void => "void".into(),
            Self::U8 => "u8".into(),
            Self::U16 => "u16".into(),
            Self::U32 => "u32".into(),
            Self::U64 => "u64".into(),
            Self::Usize => "size".into(),
            Self::I8 => "i8".into(),
            Self::I16 => "i16".into(),
            Self::I32 => "i32".into(),
            Self::I64 => "i64".into(),
            Self::Isize => "size".into(),
            Self::F32 => "f32".into(),
            Self::F64 => "f64".into(),
            Self::Bool => "bool".into(),
            Self::ConstCChar => "*const char".into(),
            Self::MutCChar => "*char".into(),
            Self::ConstVoid => "*const void".into(),
            Self::MutVoid => "*void".into(),
            Self::ConstPtr(inner) => format!("*const {}", inner.hare_ty()),
            Self::MutPtr(inner) => format!("*{}", inner.hare_ty()),
        }
    }

    pub fn needs_stddef(&self) -> bool {
        matches!(self, Self::Usize | Self::Isize)
            || matches!(self, Self::ConstPtr(inner) | Self::MutPtr(inner) if inner.needs_stddef())
    }

    pub fn needs_stdbool(&self) -> bool {
        matches!(self, Self::Bool)
            || matches!(self, Self::ConstPtr(inner) | Self::MutPtr(inner) if inner.needs_stdbool())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: FfiType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// Discovery markers (`_abi_version`, `_version`, `_name`).
    Marker,
    /// Hand-written known enrichment (rx4 / sha2).
    Enrichment,
    /// Re-export of an upstream `extern "C"` symbol (same name).
    UpstreamExternC,
    /// Auto-generated wrap of a plain Rust `pub fn`.
    AutoWrap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFn {
    pub export_name: String,
    /// Path used in the façade body, e.g. `libm::sqrt` or empty for markers.
    pub rust_callee: Option<String>,
    pub params: Vec<Param>,
    pub ret: FfiType,
    pub kind: ExportKind,
    pub is_unsafe: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub exports: Vec<ExportFn>,
    pub skipped_generics: usize,
    pub skipped_async: usize,
    pub skipped_unfriendly: usize,
    pub skipped_impl_methods: usize,
    pub source_root: Option<PathBuf>,
}

fn crate_ident(name: &str) -> String {
    name.replace('-', "_")
}

/// Parse a single Rust type token into [`FfiType`].
pub fn parse_ffi_type(raw: &str) -> Option<FfiType> {
    let t = raw
        .trim()
        .trim_start_matches("mut ")
        .trim()
        .replace(' ', "");
    let t = t.as_str();
    if matches!(t, "f16" | "f128" | "str" | "String" | "char")
        || t.contains('<')
        || t.contains('(')
        || t.contains('[')
        || t.contains('&')
        || t.contains("impl")
    {
        return None;
    }
    Some(match t {
        "()" | "void" => FfiType::Void,
        "u8" | "core::ffi::c_uchar" | "std::os::raw::c_uchar" | "libc::c_uchar" => FfiType::U8,
        "u16" => FfiType::U16,
        "u32" | "core::ffi::c_uint" | "std::os::raw::c_uint" | "libc::c_uint" => FfiType::U32,
        "u64" => FfiType::U64,
        "usize" | "core::ffi::c_size_t" => FfiType::Usize,
        "i8" => FfiType::I8,
        "i16" => FfiType::I16,
        "i32" | "core::ffi::c_int" | "std::os::raw::c_int" | "libc::c_int" => FfiType::I32,
        "i64" => FfiType::I64,
        "isize" | "core::ffi::c_ssize_t" => FfiType::Isize,
        "f32" | "core::ffi::c_float" | "std::os::raw::c_float" => FfiType::F32,
        "f64" | "core::ffi::c_double" | "std::os::raw::c_double" => FfiType::F64,
        "bool" => FfiType::Bool,
        "*constc_char"
        | "*constcore::ffi::c_char"
        | "*conststd::os::raw::c_char"
        | "*constlibc::c_char"
        | "*consti8" => FfiType::ConstCChar,
        "*mutc_char"
        | "*mutcore::ffi::c_char"
        | "*mutstd::os::raw::c_char"
        | "*mutlibc::c_char"
        | "*muti8" => FfiType::MutCChar,
        "*constc_void"
        | "*constcore::ffi::c_void"
        | "*conststd::os::raw::c_void"
        | "*constlibc::c_void" => FfiType::ConstVoid,
        "*mutc_void"
        | "*mutcore::ffi::c_void"
        | "*mutstd::os::raw::c_void"
        | "*mutlibc::c_void" => FfiType::MutVoid,
        other if other.starts_with("*const") => {
            let inner = parse_ffi_type(&other["*const".len()..])?;
            FfiType::ConstPtr(Box::new(inner))
        }
        other if other.starts_with("*mut") => {
            let inner = parse_ffi_type(&other["*mut".len()..])?;
            FfiType::MutPtr(Box::new(inner))
        }
        _ => return None,
    })
}

fn split_params(params: &str) -> Option<Vec<Param>> {
    let params = params.trim();
    if params.is_empty() {
        return Some(vec![]);
    }
    let mut out = Vec::new();
    for (idx, part) in split_top_level(params, ',').into_iter().enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // self receivers → method (skip at call site)
        if part == "self"
            || part.starts_with("&self")
            || part.starts_with("&mut self")
            || part.starts_with("self:")
            || part.starts_with("mut self")
        {
            return None;
        }
        let (name, ty_raw) = if let Some((n, t)) = part.split_once(':') {
            let n = n.trim().trim_start_matches("mut ").trim();
            if n.is_empty() {
                return None;
            }
            (n.to_string(), t.trim())
        } else {
            // unnamed — synthesize
            (format!("arg{idx}"), part)
        };
        let ty = parse_ffi_type(ty_raw)?;
        // Avoid C/Rust keyword collisions in generated headers
        let name = match name.as_str() {
            "type" | "in" | "out" | "string" | "mod" | "fn" | "let" | "pub" => {
                format!("{name}_")
            }
            _ => name,
        };
        out.push(Param { name, ty });
    }
    Some(out)
}

fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' | '[' | '<' => {
                depth += 1;
                cur.push(ch);
            }
            ')' | ']' | '>' => {
                depth -= 1;
                cur.push(ch);
            }
            c if c == sep && depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Scan a crate source root (`Cargo.toml` + `src/`).
pub fn scan_crate_sources(crate_root: &Path, package_name: &str) -> ScanReport {
    let mut report = ScanReport {
        source_root: Some(crate_root.to_path_buf()),
        ..Default::default()
    };
    let src = crate_root.join("src");
    if !src.is_dir() {
        return report;
    }
    let safe = crate_ident(package_name);
    let mut seen = BTreeSet::new();
    let root_api = collect_root_api_names(crate_root);
    let walker = walkdir::WalkDir::new(&src)
        .into_iter()
        .filter_map(|e| e.ok());
    for ent in walker {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        scan_file_text(&text, &safe, &mut report, &mut seen);
    }
    if !root_api.is_empty() {
        let before = report.exports.len();
        report.exports.retain(|e| match e.kind {
            ExportKind::UpstreamExternC => true,
            ExportKind::AutoWrap => e
                .rust_callee
                .as_deref()
                .and_then(|c| c.rsplit("::").next())
                .is_some_and(|n| root_api.contains(n)),
            _ => true,
        });
        let dropped = before.saturating_sub(report.exports.len());
        report.skipped_unfriendly += dropped;
    }
    report
}

/// Names reachable at the crate root via `pub use` / `pub fn` in `lib.rs`.
/// Used to avoid wrapping `pub` items that are only `pub(crate)` re-exported.
fn collect_root_api_names(crate_root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let walker = walkdir::WalkDir::new(crate_root.join("src"))
        .into_iter()
        .filter_map(|e| e.ok());
    for ent in walker {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("pub use ") {
                extract_use_idents(t, &mut names);
            }
        }
    }
    // Also top-level pub fns in lib.rs
    if let Ok(text) = std::fs::read_to_string(crate_root.join("src/lib.rs")) {
        for line in text.lines() {
            let t = line.trim();
            if (t.starts_with("pub fn")
                || t.starts_with("pub const fn")
                || t.starts_with("pub unsafe fn")
                || t.starts_with("pub extern")
                || t.starts_with("pub unsafe extern"))
                && let Some(rest) = t.split("fn ").nth(1)
            {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.insert(name);
                }
            }
        }
    }
    names
}

fn extract_use_idents(line: &str, out: &mut BTreeSet<String>) {
    // pub use foo::bar::{a, b as c, *};
    let Some(after) = line.strip_prefix("pub use ") else {
        return;
    };
    let after = after.trim().trim_end_matches(';').trim();
    if let Some(braced) = after.rfind('{') {
        let inside = after[braced + 1..].trim().trim_end_matches('}').trim();
        for part in inside.split(',') {
            let part = part.trim();
            if part.is_empty() || part == "*" {
                continue;
            }
            // `a as b` → export name b
            let ident = part.split_whitespace().last().unwrap_or(part);
            if ident != "self" && ident != "super" && ident != "crate" {
                out.insert(ident.to_string());
            }
        }
    } else {
        // pub use foo::bar;
        let ident = after.rsplit("::").next().unwrap_or(after).trim();
        if ident != "*" && !ident.is_empty() {
            out.insert(ident.to_string());
        }
    }
}

fn scan_file_text(
    text: &str,
    crate_safe: &str,
    report: &mut ScanReport,
    seen: &mut BTreeSet<String>,
) {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    let mut brace_depth = 0i32;
    let mut impl_body_depths: Vec<i32> = Vec::new();
    let mut pending_impl = false;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
            pending_impl = true;
        }

        let in_impl = !impl_body_depths.is_empty();
        let is_pub_fn = trimmed.starts_with("pub fn")
            || trimmed.starts_with("pub const fn")
            || trimmed.starts_with("pub async fn")
            || trimmed.starts_with("pub unsafe fn")
            || trimmed.starts_with("pub extern")
            || trimmed.starts_with("pub unsafe extern");

        if is_pub_fn {
            if in_impl || pending_impl {
                report.skipped_impl_methods += 1;
            } else {
                let prev_attrs_no_mangle = lines[..i].iter().rev().take(4).any(|l| {
                    let t = l.trim();
                    t.contains("#[no_mangle]") || t.contains("#[export_name")
                });
                let mut sig = trimmed.to_string();
                let mut j = i;
                while !sig.contains('{') && !sig.trim_end().ends_with(';') && j + 1 < lines.len() {
                    j += 1;
                    sig.push(' ');
                    sig.push_str(lines[j].trim());
                    if j - i > 8 {
                        break;
                    }
                }
                if let Some(export) =
                    classify_signature(&sig, crate_safe, prev_attrs_no_mangle, report)
                    && seen.insert(export.export_name.clone())
                {
                    report.exports.push(export);
                }
                // Account braces on continuation lines before the final line.
                while i < j {
                    let t = lines[i].trim();
                    let opens = t.matches('{').count() as i32;
                    let closes = t.matches('}').count() as i32;
                    brace_depth += opens - closes;
                    if pending_impl && opens > 0 {
                        impl_body_depths.push(brace_depth);
                        pending_impl = false;
                    }
                    while impl_body_depths.last().is_some_and(|d| brace_depth < *d) {
                        impl_body_depths.pop();
                    }
                    i += 1;
                }
            }
        }

        let opens = trimmed.matches('{').count() as i32;
        let closes = trimmed.matches('}').count() as i32;
        brace_depth += opens - closes;
        if pending_impl && opens > 0 {
            impl_body_depths.push(brace_depth);
            pending_impl = false;
        }
        while impl_body_depths.last().is_some_and(|d| brace_depth < *d) {
            impl_body_depths.pop();
        }

        i += 1;
    }
}

fn classify_signature(
    sig: &str,
    crate_safe: &str,
    no_mangle: bool,
    report: &mut ScanReport,
) -> Option<ExportFn> {
    let compact = sig.split_whitespace().collect::<Vec<_>>().join(" ");

    if compact.contains(" async ") || compact.starts_with("pub async") {
        report.skipped_async += 1;
        return None;
    }
    if compact.contains('<')
        && compact
            .find("fn ")
            .is_some_and(|p| compact[p..].contains('<'))
    {
        // generic params on the function
        if let Some(fn_pos) = compact.find("fn ") {
            let after = &compact[fn_pos + 3..];
            if after.trim_start().starts_with('<')
                || after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect::<String>()
                    .len()
                    < after.find('(').unwrap_or(0)
                    && after[..after.find('(').unwrap_or(0)].contains('<')
            {
                report.skipped_generics += 1;
                return None;
            }
            let name_end = after.find('(').unwrap_or(after.len());
            if after[..name_end].contains('<') {
                report.skipped_generics += 1;
                return None;
            }
        }
    }

    let is_extern_c = compact.contains("extern \"C\"")
        || compact.contains("extern \"c\"")
        || compact.contains("extern'C'");
    let is_unsafe = compact.contains("unsafe ");

    // Extract fn name + params + ret
    let fn_idx = compact.find("fn ")?;
    let after_fn = &compact[fn_idx + 3..];
    let name_end = after_fn.find('(')?;
    let name = after_fn[..name_end].trim();
    if name.is_empty() || name == "main" {
        return None;
    }
    if name.contains('<') {
        report.skipped_generics += 1;
        return None;
    }
    let rest = &after_fn[name_end..];
    let params_end = matching_paren(rest)?;
    let params_raw = &rest[1..params_end];
    let after_params = rest[params_end + 1..].trim();
    let ret_raw = if let Some(r) = after_params.strip_prefix("->") {
        r.split('{')
            .next()
            .unwrap_or(r)
            .split(';')
            .next()
            .unwrap_or("")
            .split("where")
            .next()
            .unwrap_or("")
            .trim()
    } else {
        "()"
    };
    if compact.contains("where ") {
        report.skipped_unfriendly += 1;
        return None;
    }

    let params = match split_params(params_raw) {
        Some(p) => p,
        None => {
            report.skipped_unfriendly += 1;
            return None;
        }
    };
    let ret = match parse_ffi_type(ret_raw) {
        Some(t) => t,
        None => {
            report.skipped_unfriendly += 1;
            return None;
        }
    };

    if is_extern_c && no_mangle {
        return Some(ExportFn {
            export_name: name.to_string(),
            rust_callee: Some(format!("{crate_safe}::{name}")),
            params,
            ret,
            kind: ExportKind::UpstreamExternC,
            is_unsafe,
        });
    }
    if is_extern_c {
        // extern C without no_mangle — still try under same name
        return Some(ExportFn {
            export_name: name.to_string(),
            rust_callee: Some(format!("{crate_safe}::{name}")),
            params,
            ret,
            kind: ExportKind::UpstreamExternC,
            is_unsafe,
        });
    }

    // Plain Rust fn → prefixed wrap
    let export_name = if name.starts_with(&format!("{crate_safe}_")) {
        name.to_string()
    } else {
        format!("{crate_safe}_{name}")
    };
    Some(ExportFn {
        export_name,
        rust_callee: Some(format!("{crate_safe}::{name}")),
        params,
        ret,
        kind: ExportKind::AutoWrap,
        is_unsafe,
    })
}

fn matching_paren(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    for (i, &b) in bytes.iter().enumerate() {
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

/// Locate unpacked crate sources: path dep, cargo registry, or download into cache.
pub fn locate_or_fetch_sources(
    package_name: &str,
    version: &str,
    path_hint: Option<&Path>,
    cache_dir: &Path,
) -> Option<PathBuf> {
    if let Some(p) = path_hint
        && p.join("Cargo.toml").is_file()
    {
        return Some(p.to_path_buf());
    }
    if version == "*" || version == "git" || version == "path" {
        return None;
    }
    if let Some(p) = find_cargo_registry(package_name, version) {
        return Some(p);
    }
    fetch_crates_io_crate(package_name, version, cache_dir).ok()
}

fn find_cargo_registry(name: &str, version: &str) -> Option<PathBuf> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cargo")))?;
    let src = cargo_home.join("registry").join("src");
    if !src.is_dir() {
        return None;
    }
    let needle = format!("{name}-{version}");
    for ent in std::fs::read_dir(&src).ok()?.flatten() {
        let cand = ent.path().join(&needle);
        if cand.join("Cargo.toml").is_file() {
            return Some(cand);
        }
    }
    None
}

fn fetch_crates_io_crate(name: &str, version: &str, cache_dir: &Path) -> anyhow::Result<PathBuf> {
    let dest = cache_dir.join(format!("{name}-{version}"));
    if dest.join("Cargo.toml").is_file() {
        return Ok(dest);
    }
    std::fs::create_dir_all(cache_dir)?;
    let url = format!("https://static.crates.io/crates/{name}/{name}-{version}.crate");
    let agent = crate::resolve::http::agent();
    let resp = agent.get(&url).call().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut reader = resp.into_reader();
    let crate_path = cache_dir.join(format!("{name}-{version}.crate"));
    let mut file = std::fs::File::create(&crate_path)?;
    std::io::copy(&mut reader, &mut file)?;
    // Prefer system tar (available on macOS/Linux).
    let status = std::process::Command::new("tar")
        .args([
            "xzf",
            crate_path.to_str().unwrap_or_default(),
            "-C",
            cache_dir.to_str().unwrap_or_default(),
        ])
        .status()?;
    let _ = std::fs::remove_file(&crate_path);
    if !status.success() {
        anyhow::bail!("tar extract failed for {name}-{version}");
    }
    if !dest.join("Cargo.toml").is_file() {
        anyhow::bail!("extracted crate missing Cargo.toml at {}", dest.display());
    }
    Ok(dest)
}

/// Optional cbindgen: when `cbindgen` is on PATH and the crate has `cbindgen.toml`,
/// run it into `out_header`. Returns true on success.
pub fn try_cbindgen(crate_root: &Path, out_header: &Path) -> bool {
    if !crate_root.join("cbindgen.toml").is_file() {
        return false;
    }
    let Ok(status) = std::process::Command::new("cbindgen")
        .args([
            "--crate",
            crate_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("crate"),
            "-o",
        ])
        .arg(out_header)
        .current_dir(crate_root)
        .status()
    else {
        return false;
    };
    status.success() && out_header.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalar_types() {
        assert_eq!(parse_ffi_type("f64"), Some(FfiType::F64));
        assert_eq!(parse_ffi_type("i32"), Some(FfiType::I32));
        assert_eq!(
            parse_ffi_type("*const u8"),
            Some(FfiType::ConstPtr(Box::new(FfiType::U8)))
        );
        assert!(parse_ffi_type("(f64, i32)").is_none());
        assert!(parse_ffi_type("&str").is_none());
        assert!(parse_ffi_type("Vec<u8>").is_none());
        assert!(parse_ffi_type("f16").is_none());
    }

    #[test]
    fn scans_simple_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "simple_api"
version = "0.1.0"
edition = "2021"
[lib]
path = "src/lib.rs"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            r#"
pub fn add(a: i32, b: i32) -> i32 { a + b }
pub fn sqrt_f64(x: f64) -> f64 { x }
pub async fn nope() {}
pub fn generic<T>(x: T) -> T { x }
pub fn bad(s: &str) -> usize { s.len() }
pub fn tuple(x: f64) -> (f64, i32) { (x, 0) }

#[no_mangle]
pub extern "C" fn simple_api_raw(x: u32) -> u32 { x }

impl Foo {
    pub fn method(self) -> i32 { 1 }
}
struct Foo;
"#,
        )
        .unwrap();
        let report = scan_crate_sources(root, "simple_api");
        let names: Vec<_> = report
            .exports
            .iter()
            .map(|e| e.export_name.as_str())
            .collect();
        assert!(names.contains(&"simple_api_add"), "{names:?}");
        assert!(names.contains(&"simple_api_sqrt_f64"), "{names:?}");
        assert!(names.contains(&"simple_api_raw"), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("nope")));
        assert!(!names.iter().any(|n| n.contains("generic")));
        assert!(!names.iter().any(|n| n.contains("bad")));
        assert!(!names.iter().any(|n| n.contains("tuple")));
        assert!(!names.iter().any(|n| n.contains("method")));
        assert!(report.skipped_async >= 1);
        assert!(report.skipped_generics >= 1);
        assert!(report.skipped_unfriendly >= 1);
    }
}
