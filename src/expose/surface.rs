//! Shared emission of scanned/enriched FFI exports into host binders.

use super::api_scan::{ExportFn, ExportKind, FfiType, TypeAdapt};

pub fn needs_stddef(exports: &[ExportFn]) -> bool {
    exports
        .iter()
        .any(|e| e.ret.needs_stddef() || e.params.iter().any(|p| p.ty.needs_stddef()))
}

pub fn needs_stdbool(exports: &[ExportFn]) -> bool {
    exports
        .iter()
        .any(|e| e.ret.needs_stdbool() || e.params.iter().any(|p| p.ty.needs_stdbool()))
}

pub fn needs_c_char(exports: &[ExportFn]) -> bool {
    fn hit(t: &FfiType) -> bool {
        matches!(
            t,
            FfiType::ConstCChar
                | FfiType::MutCChar
                | FfiType::ConstVoid
                | FfiType::MutVoid
                | FfiType::ConstPtr(_)
                | FfiType::MutPtr(_)
        ) || matches!(t, FfiType::ConstPtr(inner) | FfiType::MutPtr(inner) if hit(inner))
    }
    exports
        .iter()
        .any(|e| hit(&e.ret) || e.params.iter().any(|p| hit(&p.ty)))
}

fn join_params(
    params: &[super::api_scan::Param],
    sep: &str,
    fmt: impl Fn(&super::api_scan::Param) -> String,
) -> String {
    params.iter().map(fmt).collect::<Vec<_>>().join(sep)
}

fn csharp_param_ty(p: &super::api_scan::Param) -> String {
    match p.adapt {
        TypeAdapt::StrSlice | TypeAdapt::ByteSlice => "IntPtr".into(),
        _ => p.ty.csharp_ty(),
    }
}

fn v_c_param(p: &super::api_scan::Param) -> String {
    format!("{} {}", v_param_name(&p.name), p.ty.v_ty())
}

fn v_param_name(name: &str) -> String {
    if v_ident_ok(name) {
        return name.to_string();
    }
    let tagged = format!("{name}_");
    if v_ident_ok(&tagged) {
        tagged
    } else {
        format!("p_{name}")
    }
}

/// Emit C function declarations (no includes/guards).
pub fn emit_c_decls(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = if e.params.is_empty() {
            "void".to_string()
        } else {
            join_params(&e.params, ", ", |p| format!("{} {}", p.ty.c_ty(), p.name))
        };
        let comment = match e.kind {
            ExportKind::Marker => "/* marker */ ",
            ExportKind::Enrichment => "/* enrichment */ ",
            ExportKind::UpstreamExternC => "/* upstream extern \"C\" */ ",
            ExportKind::AutoWrap => "/* auto-wrap */ ",
        };
        out.push_str(&format!(
            "{comment}{} {}({});\n",
            e.ret.c_ty(),
            e.export_name,
            params
        ));
    }
    out
}

pub fn emit_zig_externs(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, ", ", |p| {
            format!("{}: {}", p.name, p.ty.zig_ty())
        });
        let ret = e.ret.zig_ty();
        out.push_str(&format!(
            "pub extern \"c\" fn {}({}) {};\n",
            e.export_name, params, ret
        ));
    }
    out
}

pub fn emit_nim_procs(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, "; ", |p| {
            format!("{}: {}", p.name, p.ty.nim_ty())
        });
        let ret = match &e.ret {
            FfiType::Void => String::new(),
            t => format!(": {}", t.nim_ty()),
        };
        out.push_str(&format!(
            "proc {}*({params}){ret} {{.importc, cdecl.}}\n",
            e.export_name
        ));
    }
    out
}

pub fn emit_csharp_dllimports(exports: &[ExportFn], lib_name: &str) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, ", ", |p| {
            format!("{} {}", csharp_param_ty(p), p.name)
        });
        out.push_str(&format!(
            "\n    [DllImport(\"{lib_name}\", CallingConvention = CallingConvention.Cdecl)]\n\
             public static extern {} {}({params});\n",
            e.ret.csharp_ty(),
            e.export_name
        ));
    }
    out
}

pub fn emit_d_externs(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, ", ", |p| format!("{} {}", p.ty.d_ty(), p.name));
        out.push_str(&format!(
            "    {} {}({});\n",
            e.ret.d_ty(),
            e.export_name,
            params
        ));
    }
    out
}

/// Rust façade body for auto-wrap / upstream re-exports (not markers).
pub fn emit_rust_wrappers(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    let needs_c_void = exports.iter().any(|e| {
        fn hit(t: &FfiType) -> bool {
            matches!(t, FfiType::ConstVoid | FfiType::MutVoid)
                || matches!(t, FfiType::ConstPtr(i) | FfiType::MutPtr(i) if hit(i))
        }
        matches!(e.kind, ExportKind::AutoWrap | ExportKind::UpstreamExternC)
            && (hit(&e.ret) || e.params.iter().any(|p| hit(&p.ty)))
    });
    let needs_non_null = exports.iter().any(|e| {
        matches!(e.kind, ExportKind::AutoWrap | ExportKind::UpstreamExternC)
            && (matches!(e.ret_adapt, TypeAdapt::NonNull | TypeAdapt::OptionNonNull)
                || e.params
                    .iter()
                    .any(|p| matches!(p.adapt, TypeAdapt::NonNull | TypeAdapt::OptionNonNull)))
    });
    let needs_non_zero = exports.iter().any(|e| {
        matches!(e.kind, ExportKind::AutoWrap | ExportKind::UpstreamExternC)
            && (matches!(e.ret_adapt, TypeAdapt::NonZero)
                || e.params
                    .iter()
                    .any(|p| matches!(p.adapt, TypeAdapt::NonZero)))
    });
    if needs_c_void {
        out.push_str("use std::os::raw::c_void;\n");
    }
    if needs_non_null {
        out.push_str("use std::ptr::NonNull;\n");
    }
    if needs_non_zero {
        out.push_str(
            "use std::num::{NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI8, NonZeroIsize, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU8, NonZeroUsize};\n",
        );
    }
    for e in exports {
        if matches!(e.kind, ExportKind::Marker | ExportKind::Enrichment) {
            continue;
        }
        // Upstream `#[no_mangle] extern "C"` items already exist in the linked
        // surface; redefining them here would shadow the callee and self-recurse.
        if e.kind == ExportKind::UpstreamExternC
            && e.rust_callee
                .as_deref()
                .and_then(|c| c.rsplit("::").next())
                .is_some_and(|callee| callee == e.export_name)
        {
            continue;
        }
        let Some(callee) = e.rust_callee.as_deref() else {
            continue;
        };
        let params_sig = e
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.rust_ty()))
            .collect::<Vec<_>>()
            .join(", ");
        let early = early_return_expr(&e.ret);
        let mut pre = String::new();
        let mut arg_exprs = Vec::new();
        let mut skip_next = false;
        for (pi, p) in e.params.iter().enumerate() {
            if skip_next {
                skip_next = false;
                continue;
            }
            match p.adapt {
                TypeAdapt::Identity => arg_exprs.push(p.name.clone()),
                TypeAdapt::OptionPtr => arg_exprs.push(format!(
                    "if {n}.is_null() {{ None }} else {{ Some({n}) }}",
                    n = p.name
                )),
                TypeAdapt::OptionNonNull => {
                    arg_exprs.push(format!("NonNull::new({})", p.name));
                }
                TypeAdapt::NonNull => {
                    pre.push_str(&format!(
                        "let Some({n}) = NonNull::new({n}) else {{ return {early}; }};\n    ",
                        n = p.name
                    ));
                    arg_exprs.push(p.name.clone());
                }
                TypeAdapt::NonZero => {
                    let nz = nonzero_type_name(&p.ty);
                    pre.push_str(&format!(
                        "let Some({n}) = {nz}::new({n}) else {{ return {early}; }};\n    ",
                        n = p.name
                    ));
                    arg_exprs.push(p.name.clone());
                }
                TypeAdapt::StrSlice => {
                    let len = e.params.get(pi + 1).map(|q| q.name.as_str()).unwrap_or("0");
                    pre.push_str(&format!(
                        "let {n} = if {n}.is_null() {{\n        if {len} == 0 {{\n            \"\"\n        }} else {{\n            return {early};\n        }}\n    }} else if {len} == 0 {{\n        \"\"\n    }} else {{\n        match std::str::from_utf8(unsafe {{ std::slice::from_raw_parts({n}, {len}) }}) {{\n            Ok(s) => s,\n            Err(_) => return {early},\n        }}\n    }};\n    ",
                        n = p.name,
                        len = len,
                        early = early,
                    ));
                    arg_exprs.push(p.name.clone());
                    skip_next = true;
                }
                TypeAdapt::ByteSlice => {
                    let len = e.params.get(pi + 1).map(|q| q.name.as_str()).unwrap_or("0");
                    pre.push_str(&format!(
                        "let {n} = if {n}.is_null() {{\n        if {len} == 0 {{\n            &[][..]\n        }} else {{\n            return {early};\n        }}\n    }} else if {len} == 0 {{\n        &[][..]\n    }} else {{\n        unsafe {{ std::slice::from_raw_parts({n}, {len}) }}\n    }};\n    ",
                        n = p.name,
                        len = len,
                        early = early,
                    ));
                    arg_exprs.push(p.name.clone());
                    skip_next = true;
                }
            }
        }
        let args = arg_exprs.join(", ");
        let call = if e.is_unsafe {
            format!("unsafe {{ {callee}({args}) }}")
        } else {
            format!("{callee}({args})")
        };
        let body_core = match e.ret_adapt {
            TypeAdapt::Identity | TypeAdapt::StrSlice | TypeAdapt::ByteSlice => call,
            TypeAdapt::OptionPtr => {
                let null = null_lit(&e.ret);
                format!("{call}.unwrap_or({null})")
            }
            TypeAdapt::OptionNonNull => {
                let null = null_lit(&e.ret);
                format!("{call}.map(|p| p.as_ptr()).unwrap_or({null})")
            }
            TypeAdapt::NonNull => format!("{call}.as_ptr()"),
            TypeAdapt::NonZero => format!("{call}.get()"),
        };
        let body = format!("{pre}{body_core}");
        let ret = e.ret.rust_ty();
        let unsafe_kw = if e.is_unsafe { "unsafe " } else { "" };
        let ret_arrow = if matches!(e.ret, FfiType::Void) {
            String::new()
        } else {
            format!(" -> {ret}")
        };
        out.push_str(&format!(
            "\n#[no_mangle]\npub {unsafe_kw}extern \"C\" fn {}({params_sig}){ret_arrow} {{\n    {body}\n}}\n",
            e.export_name
        ));
    }
    out
}

fn null_lit(ty: &FfiType) -> &'static str {
    match ty {
        FfiType::MutCChar | FfiType::MutVoid | FfiType::MutPtr(_) => "std::ptr::null_mut()",
        _ => "std::ptr::null()",
    }
}

fn early_return_expr(ty: &FfiType) -> String {
    match ty {
        FfiType::Void => String::new(),
        FfiType::Bool => "false".into(),
        FfiType::ConstCChar | FfiType::ConstVoid | FfiType::ConstPtr(_) => {
            "std::ptr::null()".into()
        }
        FfiType::MutCChar | FfiType::MutVoid | FfiType::MutPtr(_) => "std::ptr::null_mut()".into(),
        FfiType::F32 | FfiType::F64 => "0.0".into(),
        _ => "0".into(),
    }
}

fn nonzero_type_name(ty: &FfiType) -> &'static str {
    match ty {
        FfiType::U8 => "NonZeroU8",
        FfiType::U16 => "NonZeroU16",
        FfiType::U32 => "NonZeroU32",
        FfiType::U64 => "NonZeroU64",
        FfiType::Usize => "NonZeroUsize",
        FfiType::I8 => "NonZeroI8",
        FfiType::I16 => "NonZeroI16",
        FfiType::I32 => "NonZeroI32",
        FfiType::I64 => "NonZeroI64",
        FfiType::Isize => "NonZeroIsize",
        _ => "NonZeroU32",
    }
}

pub fn emit_v_fns(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, ", ", v_c_param);
        let ret = match &e.ret {
            FfiType::Void => String::new(),
            t => format!(" {}", t.v_ty()),
        };
        out.push_str(&format!("fn C.{}({params}){ret}\n", e.export_name));
    }
    out
}

/// Convenience `pub fn` wrappers that rewrite to `C.<export>(…)` so V hosts can
/// call `pkg.add(…)` without relying on `#include` alone.
pub fn emit_v_pub_wrappers(exports: &[ExportFn], crate_safe: &str) -> String {
    use std::collections::BTreeSet;
    let mut out = String::new();
    let mut used = BTreeSet::new();
    used.insert("package_name".into());
    used.insert("package_version".into());
    used.insert("native_lib".into());
    used.insert("native_dir".into());
    used.insert("source_root".into());
    let prefix = format!("{crate_safe}_");
    for e in exports {
        let stripped = e
            .export_name
            .strip_prefix(&prefix)
            .unwrap_or(&e.export_name);
        let Some(base) = v_pub_ident(stripped).or_else(|| v_pub_ident(&e.export_name)) else {
            continue;
        };
        let mut pub_name = base.clone();
        let mut n = 2u32;
        while !used.insert(pub_name.clone()) {
            pub_name = format!("{base}_{n}");
            n += 1;
        }
        let params_sig = join_params(&e.params, ", ", v_c_param);
        let args = e
            .params
            .iter()
            .map(|p| v_param_name(&p.name))
            .collect::<Vec<_>>()
            .join(", ");
        if e.params.is_empty()
            && matches!(e.ret, FfiType::ConstCChar)
            && (pub_name == "version" || pub_name == "name")
        {
            out.push_str(&format!(
                "pub fn {pub_name}() string {{\n\
                 \treturn unsafe {{ cstring_to_vstring(C.{}()) }}\n\
                 }}\n\n",
                e.export_name
            ));
            continue;
        }
        match &e.ret {
            FfiType::Void => out.push_str(&format!(
                "pub fn {pub_name}({params_sig}) {{\n\
                 \tC.{}({args})\n\
                 }}\n\n",
                e.export_name
            )),
            t => out.push_str(&format!(
                "pub fn {pub_name}({params_sig}) {} {{\n\
                 \treturn C.{}({args})\n\
                 }}\n\n",
                t.v_ty(),
                e.export_name
            )),
        }
    }
    out
}

fn v_pub_ident(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    if v_ident_ok(s) {
        return Some(s.to_string());
    }
    let tagged = format!("{s}_");
    if v_ident_ok(&tagged) {
        Some(tagged)
    } else {
        None
    }
}

fn v_ident_ok(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !matches!(
        s,
        "module"
            | "fn"
            | "pub"
            | "mut"
            | "return"
            | "if"
            | "else"
            | "for"
            | "in"
            | "or"
            | "and"
            | "not"
            | "is"
            | "as"
            | "type"
            | "struct"
            | "enum"
            | "interface"
            | "union"
            | "import"
            | "const"
            | "static"
            | "unsafe"
            | "go"
            | "spawn"
            | "select"
            | "match"
            | "lock"
            | "shared"
            | "atomic"
            | "asm"
            | "assert"
            | "break"
            | "continue"
            | "defer"
            | "goto"
            | "nil"
            | "true"
            | "false"
            | "none"
            | "sizeof"
            | "typeof"
            | "isreftype"
            | "dump"
            | "likely"
            | "unlikely"
    )
}

pub fn emit_odin_foreigns(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, ", ", |p| {
            format!("{}: {}", p.name, p.ty.odin_ty())
        });
        let ret = match &e.ret {
            FfiType::Void => String::new(),
            t => format!(" -> {}", t.odin_ty()),
        };
        out.push_str(&format!("	{} :: proc({params}){ret} ---\n", e.export_name));
    }
    out
}

pub fn emit_hare_fns(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = join_params(&e.params, ", ", |p| {
            format!("{}: {}", p.name, p.ty.hare_ty())
        });
        let ret = e.ret.hare_ty();
        out.push_str(&format!(
            "export @symbol(\"{}\") fn {}({params}) {ret};\n",
            e.export_name, e.export_name
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expose::api_scan::{ExportKind, Param};

    fn slice_export() -> ExportFn {
        ExportFn {
            export_name: "demo_greet".into(),
            rust_callee: Some("demo::greet".into()),
            params: vec![
                Param {
                    name: "s".into(),
                    ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
                    adapt: TypeAdapt::StrSlice,
                },
                Param {
                    name: "s_len".into(),
                    ty: FfiType::Usize,
                    adapt: TypeAdapt::Identity,
                },
            ],
            ret: FfiType::Usize,
            ret_adapt: TypeAdapt::Identity,
            kind: ExportKind::AutoWrap,
            is_unsafe: false,
        }
    }

    fn bytes_export() -> ExportFn {
        ExportFn {
            export_name: "demo_digest".into(),
            rust_callee: Some("demo::digest".into()),
            params: vec![
                Param {
                    name: "b".into(),
                    ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
                    adapt: TypeAdapt::ByteSlice,
                },
                Param {
                    name: "b_len".into(),
                    ty: FfiType::Usize,
                    adapt: TypeAdapt::Identity,
                },
            ],
            ret: FfiType::Usize,
            ret_adapt: TypeAdapt::Identity,
            kind: ExportKind::AutoWrap,
            is_unsafe: false,
        }
    }

    #[test]
    fn host_binders_emit_ptr_len_for_slices() {
        let exports = [slice_export(), bytes_export()];

        let c = emit_c_decls(&exports);
        assert!(c.contains("const uint8_t * s, size_t s_len"), "{c}");
        assert!(c.contains("const uint8_t * b, size_t b_len"), "{c}");

        let zig = emit_zig_externs(&exports);
        assert!(zig.contains("s: [*]const u8, s_len: usize"), "{zig}");
        assert!(zig.contains("b: [*]const u8, b_len: usize"), "{zig}");

        let nim = emit_nim_procs(&exports);
        assert!(nim.contains("s: ptr uint8; s_len: csize_t"), "{nim}");
        assert!(nim.contains("b: ptr uint8; b_len: csize_t"), "{nim}");

        let d = emit_d_externs(&exports);
        assert!(d.contains("const(ubyte)* s, size_t s_len"), "{d}");
        assert!(d.contains("const(ubyte)* b, size_t b_len"), "{d}");

        let v = emit_v_fns(&exports);
        assert!(
            v.contains("fn C.demo_greet(s &u8, s_len usize) usize"),
            "{v}"
        );
        assert!(
            v.contains("fn C.demo_digest(b &u8, b_len usize) usize"),
            "{v}"
        );

        let wrap = emit_v_pub_wrappers(&exports, "demo");
        assert!(
            wrap.contains("pub fn greet(s &u8, s_len usize) usize"),
            "{wrap}"
        );
        assert!(wrap.contains("return C.demo_greet(s, s_len)"), "{wrap}");
        assert!(
            wrap.contains("pub fn digest(b &u8, b_len usize) usize"),
            "{wrap}"
        );

        let odin = emit_odin_foreigns(&exports);
        assert!(odin.contains("s: ^u8, s_len: uint"), "{odin}");
        assert!(odin.contains("b: ^u8, b_len: uint"), "{odin}");

        let hare = emit_hare_fns(&exports);
        assert!(hare.contains("s: *const u8, s_len: size"), "{hare}");
        assert!(hare.contains("b: *const u8, b_len: size"), "{hare}");

        let cs = emit_csharp_dllimports(&exports, "demo_ffi");
        assert!(cs.contains("IntPtr s, UIntPtr s_len"), "{cs}");
        assert!(cs.contains("IntPtr b, UIntPtr b_len"), "{cs}");
        assert!(!cs.contains("byte[] s"), "{cs}");
    }

    #[test]
    fn rust_wrappers_adapt_str_and_byte_slices() {
        let rust = emit_rust_wrappers(&[slice_export(), bytes_export()]);
        assert!(rust.contains("from_utf8"), "{rust}");
        assert!(rust.contains("from_raw_parts(s, s_len)"), "{rust}");
        assert!(rust.contains("from_raw_parts(b, b_len)"), "{rust}");
        assert!(rust.contains("*const u8"), "{rust}");
        assert!(
            rust.contains("if s.is_null()") && rust.contains("if s_len == 0"),
            "{rust}"
        );
        assert!(
            rust.contains("return 0") || rust.contains("return 0;"),
            "null+len must early-return, got {rust}"
        );
    }

    #[test]
    fn v_pub_wrappers_rename_keywords_instead_of_dropping() {
        let e = ExportFn {
            export_name: "demo_lock".into(),
            rust_callee: None,
            params: vec![Param {
                name: "lock".into(),
                ty: FfiType::I32,
                adapt: TypeAdapt::Identity,
            }],
            ret: FfiType::I32,
            ret_adapt: TypeAdapt::Identity,
            kind: ExportKind::AutoWrap,
            is_unsafe: false,
        };
        let wrap = emit_v_pub_wrappers(&[e], "demo");
        assert!(wrap.contains("pub fn lock_("), "{wrap}");
        assert!(wrap.contains("lock_ int"), "{wrap}");
        assert!(wrap.contains("C.demo_lock(lock_)"), "{wrap}");
        assert!(!wrap.contains("pub fn lock("), "{wrap}");
    }
}
