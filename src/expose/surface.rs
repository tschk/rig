//! Shared emission of scanned/enriched FFI exports into host binders.

use super::api_scan::{ExportFn, ExportKind, FfiType};

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
    if needs_c_void {
        out.push_str("use std::os::raw::c_void;\n");
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
        let args = e
            .params
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let ret = e.ret.rust_ty();
        let unsafe_kw = if e.is_unsafe { "unsafe " } else { "" };
        let body = if e.is_unsafe || e.params.iter().any(|p| matches!(p.ty, FfiType::ConstCChar | FfiType::MutCChar | FfiType::ConstVoid | FfiType::MutVoid | FfiType::ConstPtr(_) | FfiType::MutPtr(_))) {
            // Pointer-taking wraps: call through; unsafe only when callee is unsafe.
            if e.is_unsafe {
                format!("unsafe {{ {callee}({args}) }}")
            } else {
                format!("{callee}({args})")
            }
        } else {
            format!("{callee}({args})")
        };
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
