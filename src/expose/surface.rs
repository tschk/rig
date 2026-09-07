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

/// Emit C function declarations (no includes/guards).
pub fn emit_c_decls(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = if e.params.is_empty() {
            "void".to_string()
        } else {
            e.params
                .iter()
                .map(|p| format!("{} {}", p.ty.c_ty(), p.name))
                .collect::<Vec<_>>()
                .join(", ")
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
        let params = e
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.zig_ty()))
            .collect::<Vec<_>>()
            .join(", ");
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
        let params = e
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.nim_ty()))
            .collect::<Vec<_>>()
            .join("; ");
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
        let params = e
            .params
            .iter()
            .map(|p| format!("{} {}", p.ty.csharp_ty(), p.name))
            .collect::<Vec<_>>()
            .join(", ");
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
        let params = e
            .params
            .iter()
            .map(|p| format!("{} {}", p.ty.d_ty(), p.name))
            .collect::<Vec<_>>()
            .join(", ");
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
        for p in &e.params {
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
            }
        }
        let args = arg_exprs.join(", ");
        let call = if e.is_unsafe {
            format!("unsafe {{ {callee}({args}) }}")
        } else {
            format!("{callee}({args})")
        };
        let body_core = match e.ret_adapt {
            TypeAdapt::Identity => call,
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
        let params = e
            .params
            .iter()
            .map(|p| format!("{} {}", p.ty.v_ty(), p.name))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = match &e.ret {
            FfiType::Void => String::new(),
            t => format!(" {}", t.v_ty()),
        };
        out.push_str(&format!(
            "fn C.{}({params}){ret}
",
            e.export_name
        ));
    }
    out
}

pub fn emit_odin_foreigns(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = e
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.odin_ty()))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = match &e.ret {
            FfiType::Void => String::new(),
            t => format!(" -> {}", t.odin_ty()),
        };
        out.push_str(&format!(
            "	{} :: proc({params}){ret} ---
",
            e.export_name
        ));
    }
    out
}

pub fn emit_hare_fns(exports: &[ExportFn]) -> String {
    let mut out = String::new();
    for e in exports {
        let params = e
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.hare_ty()))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = e.ret.hare_ty();
        out.push_str(&format!(
            "export @symbol(\"{}\") fn {}({params}) {ret};\n",
            e.export_name, e.export_name
        ));
    }
    out
}
