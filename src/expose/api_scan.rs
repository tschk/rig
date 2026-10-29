//! Public-API scan for simple extern-C-friendly Rust functions.
//!
//! Parses crate sources with `syn`, following the module tree from the lib
//! entry point, and collects:
//! 1. Existing `#[no_mangle] extern "C"` / `extern "C"` exports (re-exported as-is).
//! 2. Plain `pub fn` / `pub const fn` with only FFI-safe scalar/pointer types
//!    (wrapped as `{crate}_{fn}`).
//!
//! Honest skips: generics, traits/`impl` methods, `async`, tuples/arrays,
//! `&mut str` / `&mut [u8]` / `&[T]` (`T != u8`) / other refs, owned `String`/`Vec`,
//! bare `str` as a return, non-`repr(C)` structs, `f16`/`f128`, `Option<scalar>`
//! (non-niche), `dyn Trait`.
//! Wrapped: `&str` / `&[u8]` (elided / `'a` / `'static` / `'_`) as (`*const u8`,
//! `usize` len). Niches: `Option<*T>` / `Option<NonNull<_>>` / `NonNull` / `NonZero*`.
//! `extern "C-unwind"` ≈ `extern "C"`. Frontiers: `docs/AUTOWRAP.md`.

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

/// How a Rust source type maps across the C façade boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeAdapt {
    /// Same representation on both sides.
    #[default]
    Identity,
    /// `Option<*T>` ↔ nullable raw pointer.
    OptionPtr,
    /// `Option<NonNull<T>>` ↔ nullable raw pointer.
    OptionNonNull,
    /// `NonNull<T>` ↔ non-null `*mut T` (null in → early-return zero/null).
    NonNull,
    /// `NonZero*` ↔ integer (zero in → zero/nullish out).
    NonZero,
    /// `&str` ↔ (`*const u8`, `usize` len). This param is the pointer; the next
    /// synthetic `{name}_len` param supplies the byte length (UTF-8).
    StrSlice,
    /// `&[u8]` ↔ (`*const u8`, `usize` len). Same pairing as [`TypeAdapt::StrSlice`].
    ByteSlice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: FfiType,
    pub adapt: TypeAdapt,
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
    pub ret_adapt: TypeAdapt,
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
    /// `pub fn`s in private modules that are not re-exported at the crate root.
    pub skipped_private: usize,
    /// Source files `syn` could not parse (their items are not exported).
    pub parse_errors: usize,
    pub source_root: Option<PathBuf>,
}

fn crate_ident(name: &str) -> String {
    name.replace('-', "_")
}

/// Parse a single Rust type token into [`FfiType`].
pub fn parse_ffi_type(raw: &str) -> Option<FfiType> {
    parse_ffi_type_adapted(raw).map(|(ty, _)| ty)
}

/// Parse a Rust type into an FFI type plus boundary adaptation.
pub fn parse_ffi_type_adapted(raw: &str) -> Option<(FfiType, TypeAdapt)> {
    let t = raw
        .trim()
        .trim_start_matches("mut ")
        .trim()
        .replace(' ', "");
    let t = t.as_str();

    // `Option<T>` is FFI-safe iff `T` has a niche matching C nullability /
    // NonZero layout (raw pointers, NonNull, NonZero*). Scalars like
    // `Option<i32>` are rejected.
    if let Some(inner) = strip_wrapped(t, "Option") {
        return parse_option_inner(inner);
    }
    if let Some(inner) = strip_wrapped(t, "core::option::Option") {
        return parse_option_inner(inner);
    }
    if let Some(inner) = strip_wrapped(t, "std::option::Option") {
        return parse_option_inner(inner);
    }

    // `NonNull<T>` ≡ `*mut T` for C ABI purposes.
    if let Some(inner) = strip_wrapped(t, "NonNull")
        .or_else(|| strip_wrapped(t, "core::ptr::NonNull"))
        .or_else(|| strip_wrapped(t, "std::ptr::NonNull"))
    {
        let (inner_ty, _) = parse_ffi_type_adapted(inner)?;
        return Some((FfiType::MutPtr(Box::new(inner_ty)), TypeAdapt::NonNull));
    }

    // `*const ()` / `*mut ()` are common opaque void-pointer spellings; handle
    // before the blanket `(` reject (which exists to skip tuples).
    if t == "*const()" {
        return Some((FfiType::ConstVoid, TypeAdapt::Identity));
    }
    if t == "*mut()" {
        return Some((FfiType::MutVoid, TypeAdapt::Identity));
    }

    if matches!(t, "f16" | "f128" | "str" | "String" | "char")
        || t.contains('<')
        || t.contains('(')
        || t.contains('[')
        || t.contains('&')
        || t.contains("impl")
    {
        return None;
    }
    let ty = match t {
        "()" | "void" => FfiType::Void,
        "u8" | "core::ffi::c_uchar" | "std::os::raw::c_uchar" | "libc::c_uchar" => FfiType::U8,
        "u16" | "core::ffi::c_ushort" | "std::os::raw::c_ushort" | "libc::c_ushort" => FfiType::U16,
        "u32" | "core::ffi::c_uint" | "std::os::raw::c_uint" | "libc::c_uint" => FfiType::U32,
        "u64" | "core::ffi::c_ulonglong" | "std::os::raw::c_ulonglong" | "libc::c_ulonglong" => {
            FfiType::U64
        }
        "usize" | "core::ffi::c_size_t" | "libc::size_t" => FfiType::Usize,
        "i8" | "core::ffi::c_schar" | "std::os::raw::c_schar" | "libc::c_schar" => FfiType::I8,
        "i16" | "core::ffi::c_short" | "std::os::raw::c_short" | "libc::c_short" => FfiType::I16,
        "i32" | "core::ffi::c_int" | "std::os::raw::c_int" | "libc::c_int" => FfiType::I32,
        "i64" | "core::ffi::c_longlong" | "std::os::raw::c_longlong" | "libc::c_longlong" => {
            FfiType::I64
        }
        "isize" | "core::ffi::c_ssize_t" | "libc::ssize_t" => FfiType::Isize,
        "f32" | "core::ffi::c_float" | "std::os::raw::c_float" | "libc::c_float" => FfiType::F32,
        "f64" | "core::ffi::c_double" | "std::os::raw::c_double" | "libc::c_double" => FfiType::F64,
        "bool" => FfiType::Bool,
        // NonZero* share ABI with the inner integer (and with Option<NonZero*>).
        "NonZeroU8" | "core::num::NonZeroU8" | "std::num::NonZeroU8" => FfiType::U8,
        "NonZeroU16" | "core::num::NonZeroU16" | "std::num::NonZeroU16" => FfiType::U16,
        "NonZeroU32" | "core::num::NonZeroU32" | "std::num::NonZeroU32" => FfiType::U32,
        "NonZeroU64" | "core::num::NonZeroU64" | "std::num::NonZeroU64" => FfiType::U64,
        "NonZeroUsize" | "core::num::NonZeroUsize" | "std::num::NonZeroUsize" => FfiType::Usize,
        "NonZeroI8" | "core::num::NonZeroI8" | "std::num::NonZeroI8" => FfiType::I8,
        "NonZeroI16" | "core::num::NonZeroI16" | "std::num::NonZeroI16" => FfiType::I16,
        "NonZeroI32" | "core::num::NonZeroI32" | "std::num::NonZeroI32" => FfiType::I32,
        "NonZeroI64" | "core::num::NonZeroI64" | "std::num::NonZeroI64" => FfiType::I64,
        "NonZeroIsize" | "core::num::NonZeroIsize" | "std::num::NonZeroIsize" => FfiType::Isize,
        "*constc_char"
        | "*constcore::ffi::c_char"
        | "*conststd::os::raw::c_char"
        | "*conststd::ffi::c_char"
        | "*constlibc::c_char"
        | "*consti8" => FfiType::ConstCChar,
        "*mutc_char"
        | "*mutcore::ffi::c_char"
        | "*mutstd::os::raw::c_char"
        | "*mutstd::ffi::c_char"
        | "*mutlibc::c_char"
        | "*muti8" => FfiType::MutCChar,
        "*constc_void"
        | "*constcore::ffi::c_void"
        | "*conststd::os::raw::c_void"
        | "*conststd::ffi::c_void"
        | "*constlibc::c_void"
        | "*const()" => FfiType::ConstVoid,
        "*mutc_void"
        | "*mutcore::ffi::c_void"
        | "*mutstd::os::raw::c_void"
        | "*mutstd::ffi::c_void"
        | "*mutlibc::c_void"
        | "*mut()" => FfiType::MutVoid,
        other if other.starts_with("*const") => {
            let (inner, _) = parse_ffi_type_adapted(&other["*const".len()..])?;
            FfiType::ConstPtr(Box::new(inner))
        }
        other if other.starts_with("*mut") => {
            let (inner, _) = parse_ffi_type_adapted(&other["*mut".len()..])?;
            FfiType::MutPtr(Box::new(inner))
        }
        _ => return None,
    };
    let adapt = if is_nonzero_name(t) {
        TypeAdapt::NonZero
    } else {
        TypeAdapt::Identity
    };
    Some((ty, adapt))
}

fn strip_wrapped<'a>(t: &'a str, wrapper: &str) -> Option<&'a str> {
    let prefix = format!("{wrapper}<");
    let rest = t.strip_prefix(&prefix)?;
    let mut depth = 1i32;
    for (i, ch) in rest.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    let end = i + ch.len_utf8();
                    if end == rest.len() {
                        return Some(&rest[..i]);
                    }
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_option_inner(inner: &str) -> Option<(FfiType, TypeAdapt)> {
    let (ty, inner_adapt) = parse_ffi_type_adapted(inner)?;
    match (ty, inner_adapt) {
        (
            ty @ (FfiType::ConstCChar
            | FfiType::MutCChar
            | FfiType::ConstVoid
            | FfiType::MutVoid
            | FfiType::ConstPtr(_)
            | FfiType::MutPtr(_)),
            TypeAdapt::NonNull,
        ) => Some((ty, TypeAdapt::OptionNonNull)),
        (
            ty @ (FfiType::ConstCChar
            | FfiType::MutCChar
            | FfiType::ConstVoid
            | FfiType::MutVoid
            | FfiType::ConstPtr(_)
            | FfiType::MutPtr(_)),
            _,
        ) => Some((ty, TypeAdapt::OptionPtr)),
        // NonZero* already collapsed to integers; Option<NonZeroU32> ≡ u32.
        (ty, TypeAdapt::NonZero) => Some((ty, TypeAdapt::NonZero)),
        _ => None,
    }
}

fn is_nonzero_name(t: &str) -> bool {
    let base = t.rsplit("::").next().unwrap_or(t);
    base.starts_with("NonZero")
}

enum SharedBorrow {
    Str,
    Bytes,
}

/// Shared `&str` / `&[u8]` after space-stripping. Lifetimes may glue to the
/// type (`&'a str` → `&'astr`). Any `mut` after `&` is rejected.
fn parse_shared_str_or_bytes(ty_norm: &str) -> Option<SharedBorrow> {
    let rest = ty_norm.strip_prefix('&')?;
    if rest.starts_with("mut") {
        return None;
    }
    if let Some(after_lt) = rest.strip_prefix('\'') {
        if let Some(mid) = after_lt.strip_suffix("[u8]") {
            if mid.is_empty() || mid.contains("mut") {
                return None;
            }
            if mid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some(SharedBorrow::Bytes);
            }
            return None;
        }
        if let Some(mid) = after_lt
            .strip_suffix("str")
            .filter(|_| !after_lt.contains('['))
        {
            if mid.is_empty() || mid.contains("mut") {
                return None;
            }
            if mid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some(SharedBorrow::Str);
            }
            return None;
        }
        return None;
    }
    match rest {
        "str" => Some(SharedBorrow::Str),
        "[u8]" => Some(SharedBorrow::Bytes),
        _ => None,
    }
}

/// Append one Rust parameter (expanding `&str` / `&[u8]` into ptr + len).
fn push_param(out: &mut Vec<Param>, name: &str, ty_raw: &str) -> Option<()> {
    // Avoid C/Rust keyword collisions in generated headers
    let name = match name {
        "type" | "in" | "out" | "string" | "mod" | "fn" | "let" | "pub" => format!("{name}_"),
        _ => name.to_string(),
    };
    // Expand shared `&str` / `&[u8]` (any lifetime) into (ptr, len).
    // `&mut str` / `&mut [u8]` / `&[T]` (T != u8) stay unfriendly skips.
    let ty_norm = ty_raw.replace(' ', "");
    if let Some(kind) = parse_shared_str_or_bytes(&ty_norm) {
        let adapt = match kind {
            SharedBorrow::Str => TypeAdapt::StrSlice,
            SharedBorrow::Bytes => TypeAdapt::ByteSlice,
        };
        out.push(Param {
            name: name.clone(),
            ty: FfiType::ConstPtr(Box::new(FfiType::U8)),
            adapt,
        });
        out.push(Param {
            name: format!("{name}_len"),
            ty: FfiType::Usize,
            adapt: TypeAdapt::Identity,
        });
        return Some(());
    }
    let (ty, adapt) = parse_ffi_type_adapted(ty_raw)?;
    out.push(Param { name, ty, adapt });
    Some(())
}

/// Scan a crate source root (`Cargo.toml` + lib entry) by parsing it with `syn`.
///
/// Follows the module tree from the library entry point (`[lib] path`, else
/// `src/lib.rs`, else `lib.rs`) so callee paths are real (`crate::a::b::f`).
/// Exports only free `pub fn`s reachable from outside the crate: through a
/// chain of `pub mod`s, or via a `pub use` at the crate root. Skips items under
/// `#[cfg(..)]`, `impl`/trait methods, `extern` import blocks, and macro bodies.
pub fn scan_crate_sources(crate_root: &Path, package_name: &str) -> ScanReport {
    let mut scan = SynScan {
        report: ScanReport {
            source_root: Some(crate_root.to_path_buf()),
            ..Default::default()
        },
        safe: crate_ident(package_name),
        fns: Vec::new(),
        root_names: Default::default(),
        root_globs: Vec::new(),
    };
    let Some(lib) = lib_entry(crate_root) else {
        return scan.report;
    };
    let Some(file) = scan.parse(&lib) else {
        return scan.report;
    };
    let dir = lib.parent().unwrap_or(crate_root).to_path_buf();
    scan.collect_root_uses(&file.items);
    scan.walk_items(&file.items, &[], true, &dir);

    let mut seen = BTreeSet::new();
    let fns = std::mem::take(&mut scan.fns);
    for found in fns {
        let leaf = found.item.sig.ident.to_string();
        let (callee_path, export_stem) = if found.public_chain {
            let mut p = found.module.clone();
            p.push(leaf.clone());
            (p.join("::"), p.join("_"))
        } else if let Some(alias) = scan.root_alias(&found.module, &leaf) {
            (alias.clone(), alias)
        } else {
            scan.report.skipped_private += 1;
            continue;
        };
        let callee = format!("{}::{callee_path}", scan.safe);
        if let Some(export) = classify_item_fn(
            &found.item,
            &scan.safe,
            &export_stem,
            callee,
            &mut scan.report,
        ) && seen.insert(export.export_name.clone())
        {
            scan.report.exports.push(export);
        }
    }
    scan.report
}

struct FoundFn {
    module: Vec<String>,
    public_chain: bool,
    item: syn::ItemFn,
}

struct SynScan {
    report: ScanReport,
    safe: String,
    fns: Vec<FoundFn>,
    /// Root `pub use` re-exports: (source path, exported ident).
    root_names: Vec<(Vec<String>, String)>,
    /// Root `pub use path::*` globs.
    root_globs: Vec<Vec<String>>,
}

impl SynScan {
    fn parse(&mut self, path: &Path) -> Option<syn::File> {
        let text = std::fs::read_to_string(path).ok()?;
        match syn::parse_file(&text) {
            Ok(f) => Some(f),
            Err(_) => {
                self.report.parse_errors += 1;
                None
            }
        }
    }

    fn walk_items(
        &mut self,
        items: &[syn::Item],
        module: &[String],
        public_chain: bool,
        dir: &Path,
    ) {
        for item in items {
            match item {
                syn::Item::Fn(f) => {
                    if !is_public(&f.vis) || has_cfg(&f.attrs) {
                        continue;
                    }
                    self.fns.push(FoundFn {
                        module: module.to_vec(),
                        public_chain,
                        item: f.clone(),
                    });
                }
                syn::Item::Impl(imp) => {
                    self.report.skipped_impl_methods += imp
                        .items
                        .iter()
                        .filter(|i| matches!(i, syn::ImplItem::Fn(m) if is_public(&m.vis)))
                        .count();
                }
                syn::Item::Mod(m) => {
                    if has_cfg(&m.attrs) {
                        continue;
                    }
                    let name = m.ident.to_string();
                    let mut child = module.to_vec();
                    child.push(name.clone());
                    let child_public = public_chain && is_public(&m.vis);
                    let child_dir = dir.join(&name);
                    if let Some((_, items)) = &m.content {
                        self.walk_items(items, &child, child_public, &child_dir);
                        continue;
                    }
                    let file = match path_attr(&m.attrs) {
                        Some(p) => Some(dir.join(p)),
                        None => [dir.join(format!("{name}.rs")), child_dir.join("mod.rs")]
                            .into_iter()
                            .find(|p| p.is_file()),
                    };
                    let Some(file) = file else { continue };
                    let Some(parsed) = self.parse(&file) else {
                        continue;
                    };
                    let next_dir = if file.file_name().is_some_and(|f| f == "mod.rs") {
                        file.parent().unwrap_or(dir).to_path_buf()
                    } else {
                        child_dir
                    };
                    self.walk_items(&parsed.items, &child, child_public, &next_dir);
                }
                _ => {}
            }
        }
    }

    fn collect_root_uses(&mut self, items: &[syn::Item]) {
        for item in items {
            if let syn::Item::Use(u) = item
                && is_public(&u.vis)
                && !has_cfg(&u.attrs)
            {
                self.collect_use_tree(&u.tree, Vec::new());
            }
        }
    }

    fn collect_use_tree(&mut self, tree: &syn::UseTree, prefix: Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => {
                let mut next = prefix;
                let seg = p.ident.to_string();
                if !(next.is_empty() && matches!(seg.as_str(), "self" | "crate")) {
                    next.push(seg);
                }
                self.collect_use_tree(&p.tree, next);
            }
            syn::UseTree::Name(n) => {
                let mut src = prefix;
                let name = n.ident.to_string();
                src.push(name.clone());
                self.root_names.push((src, name));
            }
            syn::UseTree::Rename(r) => {
                let mut src = prefix;
                src.push(r.ident.to_string());
                self.root_names.push((src, r.rename.to_string()));
            }
            syn::UseTree::Glob(_) => self.root_globs.push(prefix),
            syn::UseTree::Group(g) => {
                for t in &g.items {
                    self.collect_use_tree(t, prefix.clone());
                }
            }
        }
    }

    /// Root-level name under which `module::leaf` is re-exported, if any.
    fn root_alias(&self, module: &[String], leaf: &str) -> Option<String> {
        let mut full = module.to_vec();
        full.push(leaf.to_string());
        self.root_names
            .iter()
            .find(|(src, _)| *src == full)
            .map(|(_, name)| name.clone())
            .or_else(|| {
                self.root_globs
                    .iter()
                    .any(|g| g.as_slice() == module)
                    .then(|| leaf.to_string())
            })
    }
}

fn lib_entry(crate_root: &Path) -> Option<PathBuf> {
    let declared = std::fs::read_to_string(crate_root.join("Cargo.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| {
            t.get("lib")?
                .get("path")?
                .as_str()
                .map(|p| crate_root.join(p))
        });
    declared
        .into_iter()
        .chain([crate_root.join("src/lib.rs"), crate_root.join("lib.rs")])
        .find(|p| p.is_file())
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

fn has_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg"))
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| {
        if !a.path().is_ident("path") {
            return None;
        }
        match &a.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        }
    })
}

/// `#[no_mangle]`, `#[unsafe(no_mangle)]`, or `#[export_name = ".."]` (edition 2024 form included).
fn exported_symbol(attrs: &[syn::Attribute], ident: &str) -> Option<String> {
    for a in attrs {
        let path = a.path();
        if path.is_ident("no_mangle") {
            return Some(ident.to_string());
        }
        if path.is_ident("export_name") {
            return path_attr_value(a);
        }
        if path.is_ident("unsafe")
            && let Ok(inner) = a.parse_args::<syn::Meta>()
        {
            if inner.path().is_ident("no_mangle") {
                return Some(ident.to_string());
            }
            if inner.path().is_ident("export_name")
                && let syn::Meta::NameValue(nv) = inner
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = nv.value
            {
                return Some(s.value());
            }
        }
    }
    None
}

fn path_attr_value(a: &syn::Attribute) -> Option<String> {
    match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    }
}

fn tokens(t: &impl quote::ToTokens) -> String {
    t.to_token_stream().to_string()
}

fn classify_item_fn(
    f: &syn::ItemFn,
    crate_safe: &str,
    export_stem: &str,
    callee: String,
    report: &mut ScanReport,
) -> Option<ExportFn> {
    let sig = &f.sig;
    if sig.asyncness.is_some() {
        report.skipped_async += 1;
        return None;
    }
    if sig
        .generics
        .params
        .iter()
        .any(|p| !matches!(p, syn::GenericParam::Lifetime(_)))
    {
        report.skipped_generics += 1;
        return None;
    }
    if sig.generics.where_clause.is_some() || sig.variadic.is_some() {
        report.skipped_unfriendly += 1;
        return None;
    }
    let name = sig.ident.to_string();
    if name == "main" {
        return None;
    }
    let abi_c = match &sig.abi {
        None => false,
        Some(abi) => match abi.name.as_ref().map(|n| n.value()) {
            None => true, // bare `extern` = "C"
            Some(n) if matches!(n.as_str(), "C" | "C-unwind") => true,
            Some(n) if n == "Rust" => false,
            Some(_) => {
                report.skipped_unfriendly += 1;
                return None;
            }
        },
    };

    let mut params = Vec::new();
    for (idx, arg) in sig.inputs.iter().enumerate() {
        let syn::FnArg::Typed(pt) = arg else {
            report.skipped_unfriendly += 1;
            return None;
        };
        let pname = match pt.pat.as_ref() {
            syn::Pat::Ident(pi) => pi.ident.to_string(),
            _ => format!("arg{idx}"),
        };
        if push_param(&mut params, &pname, &tokens(&pt.ty)).is_none() {
            report.skipped_unfriendly += 1;
            return None;
        }
    }
    let ret_raw = match &sig.output {
        syn::ReturnType::Default => "()".to_string(),
        syn::ReturnType::Type(_, t) => tokens(t),
    };
    let Some((ret, ret_adapt)) = parse_ffi_type_adapted(&ret_raw) else {
        report.skipped_unfriendly += 1;
        return None;
    };
    let is_unsafe = sig.unsafety.is_some();

    // Upstream already exports an unmangled C symbol: the façade links it through.
    if abi_c && let Some(symbol) = exported_symbol(&f.attrs, &name) {
        return Some(ExportFn {
            export_name: symbol,
            rust_callee: Some(callee),
            params,
            ret,
            ret_adapt,
            kind: ExportKind::UpstreamExternC,
            is_unsafe,
        });
    }

    let export_name = if export_stem.starts_with(&format!("{crate_safe}_")) {
        export_stem.to_string()
    } else {
        format!("{crate_safe}_{export_stem}")
    };
    Some(ExportFn {
        export_name,
        rust_callee: Some(callee),
        params,
        ret,
        ret_adapt,
        kind: ExportKind::AutoWrap,
        is_unsafe,
    })
}

/// Locate unpacked crate sources: path dep, cargo registry, or download into cache.
pub fn locate_or_fetch_sources(
    package_name: &str,
    version: &str,
    path_hint: Option<&Path>,
    cache_dir: &Path,
    expected_checksum: Option<&str>,
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
    fetch_crates_io_crate(package_name, version, cache_dir, expected_checksum).ok()
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

fn fetch_crates_io_crate(
    name: &str,
    version: &str,
    cache_dir: &Path,
    expected_checksum: Option<&str>,
) -> anyhow::Result<PathBuf> {
    if !crate::util::paths::is_valid_crate_name(name) {
        anyhow::bail!("invalid crate name `{name}`");
    }
    if !crate::util::paths::is_valid_crate_version(version) {
        anyhow::bail!("invalid crate version `{version}`");
    }
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
    drop(file);
    let looked_up = expected_checksum
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| crate::resolve::cargo::checksum_for(name, version).ok());
    verify_crate_checksum(&crate_path, looked_up.as_deref())?;
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

fn sha256_hex_file(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn verify_crate_checksum(crate_path: &Path, expected: Option<&str>) -> anyhow::Result<()> {
    let Some(expected) = expected.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let actual = sha256_hex_file(crate_path)?;
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    let _ = std::fs::remove_file(crate_path);
    anyhow::bail!(
        "checksum mismatch for {} (expected {expected}, got {actual})",
        crate_path.display()
    )
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
    fn fetch_rejects_path_injection_in_crate_name() {
        let dir = tempfile::tempdir().unwrap();
        let err = fetch_crates_io_crate("../sha2", "0.10.9", dir.path(), None).unwrap_err();
        assert!(format!("{err:#}").contains("invalid crate name"));
        let err = fetch_crates_io_crate("sha2", "../0.10.9", dir.path(), None).unwrap_err();
        assert!(format!("{err:#}").contains("invalid crate version"));
    }

    #[test]
    fn checksum_mismatch_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let crate_path = dir.path().join("pkg-1.0.0.crate");
        std::fs::write(&crate_path, b"not-a-real-crate").unwrap();
        let err = verify_crate_checksum(&crate_path, Some("deadbeef")).unwrap_err();
        assert!(format!("{err:#}").contains("checksum mismatch"));
        assert!(!crate_path.exists(), "mismatched crate should be deleted");
    }

    #[test]
    fn checksum_match_ok() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let crate_path = dir.path().join("pkg-1.0.0.crate");
        std::fs::write(&crate_path, b"abc").unwrap();
        let sum = hex::encode(Sha256::digest(b"abc"));
        verify_crate_checksum(&crate_path, Some(&sum)).unwrap();
        assert!(crate_path.exists());
    }

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
    fn parses_option_pointer_and_nonzero_niches() {
        assert_eq!(parse_ffi_type("*const c_void"), Some(FfiType::ConstVoid));
        assert_eq!(
            parse_ffi_type("Option<*mut u8>"),
            Some(FfiType::MutPtr(Box::new(FfiType::U8)))
        );
        assert_eq!(
            parse_ffi_type("Option<*const c_void>"),
            Some(FfiType::ConstVoid)
        );
        assert_eq!(
            parse_ffi_type("NonNull<u8>"),
            Some(FfiType::MutPtr(Box::new(FfiType::U8)))
        );
        assert_eq!(
            parse_ffi_type("Option<NonNull<u32>>"),
            Some(FfiType::MutPtr(Box::new(FfiType::U32)))
        );
        assert_eq!(parse_ffi_type("NonZeroU32"), Some(FfiType::U32));
        assert_eq!(
            parse_ffi_type("Option<core::num::NonZeroU64>"),
            Some(FfiType::U64)
        );
        assert_eq!(parse_ffi_type("*const ()"), Some(FfiType::ConstVoid));
        assert_eq!(parse_ffi_type("*mut ()"), Some(FfiType::MutVoid));
        assert_eq!(parse_ffi_type("c_longlong"), None); // needs path prefix or i64
        assert_eq!(parse_ffi_type("core::ffi::c_longlong"), Some(FfiType::I64));
        assert_eq!(parse_ffi_type("core::ffi::c_ushort"), Some(FfiType::U16));
        // Option around plain scalars is NOT niche-safe for C.
        assert!(parse_ffi_type("Option<i32>").is_none());
        assert!(parse_ffi_type("Option<bool>").is_none());
    }

    #[test]
    fn push_param_expands_str_and_byte_slices() {
        let mut params = Vec::new();
        for (name, ty) in [("s", "&str"), ("b", "&[u8]"), ("n", "i32")] {
            push_param(&mut params, name, ty).unwrap();
        }
        assert_eq!(params.len(), 5);
        assert_eq!(params[0].name, "s");
        assert!(matches!(params[0].adapt, TypeAdapt::StrSlice));
        assert_eq!(params[1].name, "s_len");
        assert!(matches!(params[1].ty, FfiType::Usize));
        assert_eq!(params[2].name, "b");
        assert!(matches!(params[2].adapt, TypeAdapt::ByteSlice));
        assert_eq!(params[3].name, "b_len");
        assert_eq!(params[4].name, "n");
        assert!(matches!(params[4].ty, FfiType::I32));
    }

    #[test]
    fn scans_str_and_byte_slice_params() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "slice_api"
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
pub fn greet_len(s: &str) -> usize { s.len() }
pub fn bytes_len(b: &[u8]) -> usize { b.len() }
"#,
        )
        .unwrap();
        let report = scan_crate_sources(root, "slice_api");
        let names: Vec<_> = report
            .exports
            .iter()
            .map(|e| e.export_name.as_str())
            .collect();
        assert!(names.contains(&"slice_api_greet_len"), "{names:?}");
        assert!(names.contains(&"slice_api_bytes_len"), "{names:?}");
        let greet = report
            .exports
            .iter()
            .find(|e| e.export_name == "slice_api_greet_len")
            .unwrap();
        assert_eq!(greet.params.len(), 2);
        assert!(matches!(greet.params[0].adapt, TypeAdapt::StrSlice));
    }

    #[test]
    fn scans_root_lib_rs_without_src_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "flat_api"
version = "0.1.0"
edition = "2021"
[lib]
path = "lib.rs"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }
",
        )
        .unwrap();
        let report = scan_crate_sources(root, "flat_api");
        assert!(
            report
                .exports
                .iter()
                .any(|e| e.export_name == "flat_api_add"),
            "{:?}",
            report.exports
        );
    }

    #[test]
    fn scans_option_pointer_and_c_unwind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "niche_api"
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
use std::ptr::NonNull;
use std::num::NonZeroU32;

pub fn take_opt(p: Option<*mut u8>) -> Option<*mut u8> { p }
pub fn take_nn(p: NonNull<u8>) -> u8 { unsafe { *p.as_ptr() } }
pub fn take_nz(n: NonZeroU32) -> u32 { n.get() }

#[no_mangle]
pub extern "C-unwind" fn niche_api_raw(x: u32) -> u32 { x }
"#,
        )
        .unwrap();
        let report = scan_crate_sources(root, "niche_api");
        let names: Vec<_> = report
            .exports
            .iter()
            .map(|e| e.export_name.as_str())
            .collect();
        assert!(names.contains(&"niche_api_take_opt"), "{names:?}");
        assert!(names.contains(&"niche_api_take_nn"), "{names:?}");
        assert!(names.contains(&"niche_api_take_nz"), "{names:?}");
        assert!(names.contains(&"niche_api_raw"), "{names:?}");
    }

    fn scan_lib(name: &str, files: &[(&str, &str)]) -> ScanReport {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        for (rel, text) in files {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        scan_crate_sources(root, name)
    }

    fn callees(report: &ScanReport) -> Vec<(String, String)> {
        report
            .exports
            .iter()
            .map(|e| {
                (
                    e.export_name.clone(),
                    e.rust_callee.clone().unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn scanner_ignores_test_modules_macros_comments_and_foreign_blocks() {
        let report = scan_lib(
            "tricky",
            &[(
                "src/lib.rs",
                r#"
pub fn real(x: i32) -> i32 { x }

/*
pub fn commented_out(x: i32) -> i32 { x }
*/

macro_rules! make {
    ($n:ident) => {
        pub fn $n() -> i32 { 0 }
    };
}

extern "C" {
    pub fn imported_from_c(x: i32) -> i32;
}

#[cfg(test)]
mod tests {
    pub fn test_helper(x: i32) -> i32 { x }
}
"#,
            )],
        );
        let names: Vec<_> = report
            .exports
            .iter()
            .map(|e| e.export_name.as_str())
            .collect();
        assert_eq!(names, vec!["tricky_real"], "{names:?}");
    }

    #[test]
    fn scanner_is_not_confused_by_braces_in_strings() {
        // A `}` inside a string used to close the impl early, turning the
        // associated fn below into a bogus free-function export.
        let report = scan_lib(
            "braces",
            &[(
                "src/lib.rs",
                r#"
pub struct S;
impl S {
    pub fn first(x: i32) -> i32 { let _ = "}"; x }
    pub fn assoc(y: i32) -> i32 { y }
}
pub fn free(z: i32) -> i32 { let _ = '{'; z }
"#,
            )],
        );
        let names: Vec<_> = report
            .exports
            .iter()
            .map(|e| e.export_name.as_str())
            .collect();
        assert_eq!(names, vec!["braces_free"], "{names:?}");
        assert_eq!(report.skipped_impl_methods, 2);
    }

    #[test]
    fn scanner_uses_real_module_paths_and_visibility() {
        let report = scan_lib(
            "modded",
            &[
                (
                    "src/lib.rs",
                    r#"
pub mod util;
mod hidden;
mod private_inline { pub fn unreachable(x: u8) -> u8 { x } }
pub mod inline { pub fn nested(x: u16) -> u16 { x } }
pub use hidden::helper as public_helper;
"#,
                ),
                (
                    "src/util.rs",
                    "pub fn twice(x: u32) -> u32 { x * 2 }
",
                ),
                (
                    "src/hidden.rs",
                    "pub fn helper(x: u64) -> u64 { x }
pub fn not_reexported(x: u64) -> u64 { x }
",
                ),
            ],
        );
        let got = callees(&report);
        let has = |export: &str, callee: &str| got.iter().any(|(e, c)| e == export && c == callee);
        assert!(has("modded_util_twice", "modded::util::twice"), "{got:?}");
        assert!(
            has("modded_inline_nested", "modded::inline::nested"),
            "{got:?}"
        );
        assert!(
            has("modded_public_helper", "modded::public_helper"),
            "{got:?}"
        );
        assert!(
            !got.iter()
                .any(|(e, _)| e.contains("unreachable") || e.contains("not_reexported")),
            "{got:?}"
        );
    }

    #[test]
    fn scanner_reads_long_signatures_and_lifetime_only_generics() {
        let report = scan_lib(
            "longsig",
            &[(
                "src/lib.rs",
                r#"
pub fn many(
    a: i32,
    b: i32,
    c: i32,
    d: i32,
    e: i32,
    f: i32,
    g: i32,
    h: i32,
    i: i32,
    j: i32,
) -> i32 {
    a + b + c + d + e + f + g + h + i + j
}

pub fn borrowed<'a>(s: &'a str) -> usize { s.len() }
"#,
            )],
        );
        let many = report
            .exports
            .iter()
            .find(|e| e.export_name == "longsig_many")
            .unwrap_or_else(|| panic!("{:?}", callees(&report)));
        assert_eq!(many.params.len(), 10);
        assert_eq!(many.ret, FfiType::I32);
        assert!(
            report
                .exports
                .iter()
                .any(|e| e.export_name == "longsig_borrowed"),
            "{:?}",
            callees(&report)
        );
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
pub fn greet(s: &str) -> usize { s.len() }
pub fn digest(b: &[u8]) -> usize { b.len() }
pub fn bad(s: *mut String) -> usize { 0 }
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
        assert!(names.contains(&"simple_api_greet"), "{names:?}");
        assert!(names.contains(&"simple_api_digest"), "{names:?}");
        let greet = report
            .exports
            .iter()
            .find(|e| e.export_name == "simple_api_greet")
            .unwrap();
        assert_eq!(greet.params.len(), 2);
        assert!(matches!(greet.params[0].adapt, TypeAdapt::StrSlice));
        assert!(matches!(greet.params[0].ty, FfiType::ConstPtr(_)));
        assert_eq!(greet.params[1].name, "s_len");
        assert!(matches!(greet.params[1].ty, FfiType::Usize));
        let digest = report
            .exports
            .iter()
            .find(|e| e.export_name == "simple_api_digest")
            .unwrap();
        assert_eq!(digest.params.len(), 2);
        assert!(matches!(digest.params[0].adapt, TypeAdapt::ByteSlice));
        assert_eq!(digest.params[1].name, "b_len");
        assert!(!names.iter().any(|n| n.contains("bad")));
        assert!(!names.iter().any(|n| n.contains("tuple")));
        assert!(!names.iter().any(|n| n.contains("method")));
        assert!(report.skipped_async >= 1);
        assert!(report.skipped_generics >= 1);
        assert!(report.skipped_unfriendly >= 2);
    }
}
