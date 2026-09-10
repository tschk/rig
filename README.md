# rig

Cross-language native dependency manager. Detect host lang, add libs via CLI, expose them in-process with [equilibrium-ffi](https://github.com/tschk/equilibrium).

```text
rig add --rust <crate>
rig up
rig ui <pkg>
rig dr
```

Host projects consume native libs in-process; any cargo crate can be exposed into a non-Rust host.

## Install

```bash
cargo install rigpkg
```

Or from git:

```bash
cargo install --git https://github.com/tschk/rig
```

Both install the `rig` binary.

## INIT

```bash
rig init
```

Writes `rig.toml` + empty `rig.lock`. Detects host language from markers (`Cargo.toml`, `build.zig`, `*.nimble`, ...).

## ADD

```bash
rig add --rust <crate>
rig i --cargo <crate>
rig add --zig some_lib
```

Aliases: `i`, `install` (short package-manager aliases).

Resolves the package, pins it in `rig.toml` / `rig.lock`, and auto-exposes a native in-process surface:

- Rust host <- cargo crate: adds the dep to `Cargo.toml` and generates `src/rig_bindings/<pkg>.rs` (re-export + markers).
- Zig host <- cargo crate: builds a `{crate}_ffi` cdylib facade (markers: `{crate}_abi_version` / `_version` / `_name`), writes `src/rig_bindings/<pkg>_bindings.zig`, and patches `build.zig` with `// rig-expose-begin` link markers.
- Nim host <- cargo crate: builds the same facade and writes `src/rig_bindings/<pkg>.nim` (`importc` + `passL` link hints).
- C/C++ host <- cargo crate: builds the same facade and writes `src/rig_bindings/<pkg>.h` (declarations + `-L/-l` metadata macros).
- C# host <- cargo crate: builds the same facade and writes `src/rig_bindings/<pkg>.cs` (`DllImport` P/Invoke wrappers).
- D host <- cargo crate: builds the same facade and writes `src/rig_bindings/<pkg>.d` (`extern(C)` + `pragma(lib)`).
- V host <- cargo crate: builds the same facade and writes `modules/<pkg>/<pkg>.v` (`#flag` + `fn C.` decls + `pub fn` wrappers so a host can `import pkg` and call `pkg.add(...)`).
- Odin host <- cargo crate: builds the same facade and writes `src/rig_bindings/<pkg>.odin` (`foreign import` + `foreign` block).
- Hare host <- cargo crate: builds the same facade and writes `src/rig_bindings/<pkg>.ha` (`@symbol` C ABI decls + `-L/-l` hints).
- C/C++/Zig/Nim/V/Odin/Hare/C#/D host <- path/git c|cpp|zig|nim|v|odin|hare: compiles into `target/rig/<pkg>/lib*_native.{dylib,so}` when feasible (flat sources, Makefile `$OUT`, CMake, meson, or language toolchain); clear error if the toolchain is missing. Simple C prototypes from discovered headers (and Zig `export fn` when the dep is Zig) are emitted into Nim/V/Zig/Odin/Hare/C#/D binders. Hare path/git is skipped with that error when `hare` is not on `PATH` (rig does not install a toolchain).
- Rust host <- foreign lang: generates an equilibrium-ffi `load` path stub.

Ecosystem flags (mutually exclusive): `--cargo`/`--rust`, `--zig`, `--nim`, `--c`, `--cpp`, `--v`, `--d`, `--odin`, `--hare`, `--csharp`/`--cs`.

## UP

```bash
rig up
rig upgrade <pkg>
```

## REMOVE

```bash
rig ui <pkg>
rig rm <pkg>
```

Aliases: `rm`, `ui`, `uninstall`.

## LIST / SEARCH / INFO

```bash
rig ls
rig s serde
rig search --nim jester
rig search --d vibe
rig search --csharp Newtonsoft.Json
rig search --zig http          # GitHub hints; no central Zig registry
rig search --c foo             # clear unsupported (use path:/git+)
rig show <pkg>
rig info --nim jester
rig outdated
```

Registry-backed today: cargo (crates.io), nim (nimble packages.json), d (code.dlang.org), csharp (NuGet).
zig accepts `path:` / `git+` / `https://` pins (search is hint-only).
c / cpp / v / odin / hare are path/git pins only.

## LOCK / SYNC

```bash
rig lock
rig sync
```

## CHECK / BUILD

```bash
rig dr
rig check --fix --full
rig build
```

## Host demos

- [`examples/zig-host-multi`](examples/zig-host-multi): Zig <- multiple cargo crates (`rx4` + `sha2`) via the generic facade.
- [`examples/zig-host-rx4`](examples/zig-host-rx4): Zig <- `rx4` via a thin `rx4_ffi` facade + equilibrium-ffi.
- [`examples/c-host-sha2`](examples/c-host-sha2): C <- `sha2` markers + `sha2_hash_256`/`sha2_hash_512` (`cc` + `-lsha2_ffi`).
- [`examples/c-host-libm`](examples/c-host-libm): C <- auto-wrapped `libm` (`libm_sqrt`, ...) beyond markers.

```bash
cd examples/zig-host-multi
rig sync
zig build run
```

## C ABI facade (cargo -> non-Rust hosts)

For every `rig add --rust <crate>` on a Zig (or other non-Rust) host, rig generates a thin `cdylib` under `.rig/shims/<crate>/` (unless the crate already ships `crate-type = ["cdylib"]`, in which case that library is linked through):

| Symbol | Meaning |
|--------|---------|
| `{crate}_abi_version` | Facade ABI revision (`u32`) |
| `{crate}_version` | Null-terminated version string |
| `{crate}_name` | Null-terminated crate name |

Known enrichments (today: `rx4`, `sha2`, `crc32fast`, `md-5`, `hex`, `base64`) may export additional methods beyond the markers (e.g. `rx4_prompt_smoke`, `sha2_hash_256`, `hex_encode`, `base64_encode`).

For other crates, rig auto-wraps a scanned public surface when sources are available (cargo registry / path / crates.io fetch):

- Existing `#[no_mangle] extern "C"` functions are re-exported under the same name.
- Plain `pub fn` / `pub const fn` with only FFI-safe scalar/pointer types become `{crate}_{fn}` exports (facade ABI 2).
- `&str` and `&[u8]` auto-wrap as a (`*const u8`, `usize` len) pair (explicit length; not NUL-terminated).
- Host binders (Zig/Nim/C/C#/D/V) emit the discovered declarations; V also emits `pub fn` wrappers over `C.<export>`. See `.rig/shims/<crate>/surface.json`.

### Honest limits

- Skipped: generics, `async`, `impl` methods/traits, tuples/arrays, other refs, `String`/`Vec`, `f16`/`f128`, `Option<scalar>` (non-niche), and other non-FFI-safe types.
- Wrapped slices: `&str` / `&[u8]` as pointer+len (UTF-8 checked for `&str`; invalid UTF-8 early-returns).
- Wrapped niches (safe): `Option<*T>` / `Option<NonNull<T>>` / `NonNull<T>` / `NonZero*` / `*const ()` / `*mut ()`, plus more `core::ffi::c_*` aliases; `extern "C-unwind"` treated like `extern "C"`.
- cbindgen runs only when the crate ships `cbindgen.toml` and the `cbindgen` binary is on `PATH` (provenance header only; scan remains canonical).
- Crates with `#![forbid(unsafe_code)]` still work: unsafe lives only in the generated facade.
- Auto-wrap caps at 256 symbols per crate; enrichments remain intentional for trait-heavy APIs (e.g. `sha2_hash_256`).
- Rust hosts keep the native Cargo re-export path (`src/rig_bindings/<pkg>.rs`); the cdylib facade is for cross-language hosts.

## Manifest

See committed example [`rig.toml`](rig.toml). Schema:

- `[host].language`: rust | zig | nim | c | cpp | v | d | odin | hare | csharp
- `[dependencies.<name>]`: ecosystem, version, git/path, features, expose
- `[expose].dir`: default `src/rig_bindings` (use `src/vendor` for Zig/C imports)

Lockfile: `rig.lock` (commit it).

## Detected languages

| Language | Markers |
|----------|---------|
| Rust | `Cargo.toml` |
| Zig | `build.zig`, `build.zig.zon` |
| Nim | `*.nimble`, `nimble.paths` |
| D | `dub.json`, `dub.sdl` |
| C# | `*.csproj`, `*.sln` |
| V | `v.mod` |
| Odin | `ols.json`, `*.odin` |
| Hare | `*.ha` |
| C / C++ | `CMakeLists.txt`, `meson.build`, `Makefile` + sources |

Equilibrium-supported set: V, Zig, C, C++, C#, Rust, D, Nim, Odin, Hare.

## Links

- [equilibrium](https://github.com/tschk/equilibrium)

## License

ISC

## CI

GitHub Actions runs `fmt` / `clippy -D warnings` / `cargo test` on Ubuntu + macOS (+ Windows tests).
An optional path-c-smoke job on Ubuntu exercises flat-C and CMake path/git fixtures via `scripts/ci-smoke-path-c.sh` (cmake/ninja; meson when present).
`scripts/ci-smoke-path-extra.sh` tries a Zig path fixture when `zig` is on `PATH` (skipped otherwise). Nim/V/Odin/Hare e2e compile smokes run locally when those toolchains are present (`cargo test` skips them quietly on stock CI).
