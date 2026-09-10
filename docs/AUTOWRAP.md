# Auto-wrap frontiers

Design notes for rig’s cargo-façade **auto-wrap** path (`src/expose/api_scan.rs` →
`surface.rs` → host binders). Aimed at box volume-impl: ship one frontier at a
time with unit tests + one smoke when a host toolchain is present.

Public copy: describe ABI facts only — never wax / oil / fx / telekinesis.

## Current state (shipped / in WIP)

| Rust surface | C façade ABI | Notes |
|---|---|---|
| Scalars (`bool`, ints, floats except `f16`/`f128`), `()` | identity | Early-return zero/nullish on niche failure |
| `*const T` / `*mut T`, `*const ()` / `*mut ()` | raw pointers | |
| `Option<*T>`, `Option<NonNull<T>>`, `NonNull<T>`, `NonZero*` | nullable / non-null ptr or int | `TypeAdapt::{OptionPtr,OptionNonNull,NonNull,NonZero}` |
| `extern "C"` / `extern "C-unwind"` `#[no_mangle]` | re-export same name | UpstreamExternC |
| Plain `pub fn` / `pub const fn` (FFI-safe args) | `{crate}_{fn}` | AutoWrap, façade ABI 2 |
| `&str` | `const uint8_t *p, size_t p_len` | `TypeAdapt::StrSlice`; UTF-8 checked; null/0 → `""`; invalid UTF-8 → early-return |
| `&[u8]` | `const uint8_t *p, size_t p_len` | `TypeAdapt::ByteSlice`; null/0 → `&[]` |

Host binders (C / Zig / Nim / C# / D / V / Odin / Hare) emit the expanded
ptr+len pairs. **V** also emits `pub fn` wrappers over `C.<export>` so hosts can
`import pkg` and call `pkg.add(…)` / `pkg.abi_version()` (module layout still
needs a discoverable path — see V notes below).

Surface inventory: `.rig/shims/<crate>/surface.json`.

## Permanent skip list (do not auto-wrap)

| Pattern | Why |
|---|---|
| Generics / `impl Trait` / TAIT | Instantiation set unknown at façade build |
| `async fn` / `impl Future` | Needs runtime; not C ABI |
| `impl` methods / trait items | Need `Self` / vtable; see trait-object limits |
| Tuples, fixed arrays `[T; N]`, bare `str` / `String` / `Vec<_>` (owned) | Layout / ownership / allocator — see return frontier |
| `&T` / `&mut T` to non-slice user types | No stable C layout without `repr(C)` + explicit policy |
| `Option<scalar>` (non-niche) | Ambiguous null/zero; niches only when pointer/NonZero |
| `f16` / `f128` | Host binder coverage uneven |
| `dyn Trait` / fat pointers | See § Trait objects |

Skips are counted on `ScanReport` (`skipped_generics`, `skipped_async`,
`skipped_unfriendly`, `skipped_impl_methods`).

---

## Frontier A — `&mut [u8]` / `&mut str` (next hard piece)

### Recommended ABI

```text
&mut [u8]  →  uint8_t *p, size_t p_len     (TypeAdapt::MutByteSlice)
&mut str   →  SKIP for now (UTF-8 in-place mutation is rare + easy to corrupt)
```

Semantics:

- Null pointer + `len == 0` → empty mutable slice `&mut []`.
- Null + `len != 0` → early-return (same family as NonNull failure).
- Non-null → `std::slice::from_raw_parts_mut(p, len)`.
- Length is **capacity of the buffer the caller prepared**; Rust callee may
  shorten logical use but **must not** expect to grow past `p_len` (no out-len
  param in v1).
- Do **not** add an out-len parameter in v1 (keeps host binders one arity family
  with shared StrSlice/ByteSlice pairing: adapt consumes param `i`, len is `i+1`).

### Rejected alternatives

- `*mut u8` only (no len): forces strlen-style bugs for binary data.
- In/out `size_t *len`: useful later for compress-style APIs; defer to Frontier C
  sibling once MutByteSlice ships.

### Host binder checklist

| Host | Emission |
|---|---|
| C / header | `uint8_t * p, size_t p_len` |
| Zig | `p: [*]u8, p_len: usize` |
| Nim | `p: ptr uint8; p_len: csize_t` |
| V `fn C.` | `p &u8, p_len usize` (V has no distinct mut ptr in C decls — document as writable) |
| V `pub fn` | same arity; pass-through to `C.` |
| D | `ubyte* p, size_t p_len` |
| Odin | `p: [^]u8, p_len: int` (or `uint`) |
| Hare | `p: *u8, p_len: size` |
| C# | `IntPtr p, UIntPtr p_len` (or `nuint`) |

### Tests

- `api_scan`: fixture `pub fn fill(buf: &mut [u8])` → MutByteSlice + len param.
- `surface::emit_rust_wrappers`: rebuilds `&mut [u8]` before call.
- Host emit smoke: C/Zig/Nim/V/D contain ptr+len.
- Skip: `&mut str` still increments `skipped_unfriendly` until a later RFC.

---

## Frontier B — `&[T]` for FFI-safe `T`

Only when `T` is a **C-repr scalar** already in `FfiType` (integers, floats,
`bool`, raw pointers). Not for `T: Copy` user structs until `repr(C)` scan exists.

```text
&[u32]  →  const uint32_t *p, size_t p_len
&[T]    →  const T *p, size_t p_len     (T from FfiType::c_ty)
```

Alignment: document that caller must pass a pointer aligned for `T`. Façade uses
`from_raw_parts(p as *const T, len)`.

`&mut [T]` follows Frontier A once MutByteSlice lands (generalize adapt to
`MutSlice(FfiType)` / `ConstSlice(FfiType)` if the enum stays tidy).

Skip: `&[str]`, `&[String]`, slices of tuples, ZST-only edge cases beyond
`&[()]` (defer).

---

## Frontier C — returning `String` / `Vec<u8>`

### Recommended pattern (pick one): **caller-allocated out-buffer**

```text
fn describe(...) -> String
becomes:
  size_t crate_describe(..., uint8_t *out, size_t out_len);
  // return bytes written, or out_len+1 on truncation, or 0 on error
```

Why:

- No allocator handoff across CDylib boundary (macOS/Windows CRT mismatches).
- Matches existing ptr+len discipline from StrSlice.
- Host languages already pass buffers.

### Rejected

| Pattern | Why reject as default |
|---|---|
| Façade `malloc` + caller `crate_free` | Easy to leak; dual-allocator hell |
| Return `*const c_char` into static TLS/arena | Not general; thread hazards |
| Return `{ptr,len,cap}` owned by Rust until `free` | Same as malloc pair; OK as **opt-in enrichment** only |

Enrichment shims (sha2/hex/base64) may keep specialized encode-into-buffer APIs;
generic auto-wrap should use out-buffer.

Owned **inputs** (`String`, `Vec<u8>`) stay skipped unless we add an explicit
`TypeAdapt::OwnedBytes` that copies from ptr+len into a temporary `Vec` (possible
follow-up; not required for Frontier C returns).

---

## Frontier D — trait objects / `dyn Trait` (hard limits)

**Do not auto-wrap** `dyn Trait`, `Box<dyn Trait>`, `&dyn Trait`, or fat pointers.

Reasons:

1. Fat pointer = `(data, vtable)`; vtable layout is rustc-internal and
   **not** a C ABI.
2. Object safety ≠ FFI safety; methods may use generics/`Self`/async.
3. Lifetime + ownership across dlopen boundaries is a product decision, not a
   scan heuristic.

### Escape hatches

- Hand enrichment in `shim.rs` for known crates (existing pattern).
- Rust host ← foreign: equilibrium-ffi `load` stubs (`rust_host.rs`).
- Expose a **narrow C vtable struct** the crate authors maintain (`repr(C)`
  function pointers) — rig can re-export those as UpstreamExternC when
  `#[no_mangle] extern "C"` exists; it must not invent the vtable.

Volume-impl: keep skip counters; maybe surface a single doc line in generated
binder comments when skips include impl methods: `/* skipped N impl/trait items */`.

---

## Frontier E — generics / async (permanent)

Leave as skip. No ABI sketch. If a monomorphized `pub use` re-export of a
concrete function appears as a plain `pub fn` in the crate root API set, the
existing root-api filter may wrap that concrete item — that is enough.

---

## Host binder emission checklist (any new adapt)

For each new `TypeAdapt` variant:

1. Expand to one or more `Param`s in `split_params` / classify (arity).
2. `FfiType` host mappings (`c_ty`, `zig_ty`, …) for expanded params.
3. `emit_rust_wrappers` pre-call rebuild + early-return policy.
4. All `emit_*` binders + V `emit_v_pub_wrappers`.
5. Unit tests in `api_scan`, `surface`, and one shim integration if façade text changes.
6. README “Honest limits” bullet — one line, factual.
7. Update this file’s “Current state” table.

## V convenience wrappers — layout note

Generated `module {safe}` + `pub fn` wrappers are necessary but not sufficient:
V resolves imports via `modules/<name>/` or `v.mod` search paths. Smokes should
copy binder + header into `modules/<pkg>/` (see `tests/cli_smoke.rs`) **or**
document `v -path` / project `v.mod`. Prefer not inventing a second binder path
until module layout is decided; check/doctor can warn if `*.v` exists but no
`modules/<pkg>` mirror when host is V (optional follow-up).

## Doctor / check (binder freshness)

Today:

- `.rig/expose-stamp` hashes manifest deps + lock packages (`stamp.rs`).
- `rig check` verifies stamp freshness and **binder file presence** per expose dep.

Next:

- Per-dep: binder mtime vs `.rig/shims/<pkg>/surface.json` mtime (or hash of
  surface.json) so a stale binder after scan changes fails check even if stamp
  was rewritten incorrectly.
- `--fix` already calls `expose::resync_all`; keep that as the repair path.

## Testing matrix (abbreviated)

| Layer | Command / artifact |
|---|---|
| Unit | `cargo test --lib` (api_scan, surface, shim, hosts, check) |
| CLI smoke | `tests/cli_smoke.rs` gated on host tools (`v`, `zig`, …) |
| Path/git | CMake/meson/Makefile flats; Hare honest-skip without toolchain |
| Manual | `rig add --rust <crate>` in a temp host; call one wrapped fn |

## Implementation order (suggested)

1. **MutByteSlice** (`&mut [u8]`) — small, mirrors ByteSlice.
2. Check binder↔surface freshness.
3. ConstSlice for scalar `T` (`&[u32]`, …).
4. Out-buffer returns for `String`/`Vec<u8>`.
5. Trait-object documentation only (no code beyond skip comment polish).
