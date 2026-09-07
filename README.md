# rig

Cross-language native dependency manager. Detect host lang, add libs via CLI, expose them in-process with [equilibrium-ffi](https://github.com/tschk/equilibrium).

```text
rig add --rust rx4
rig up
rig ui rx4
rig dr
```

Host projects consume native libs in-process; [rotary](https://github.com/tschk/rotary) (`rx4`) is a common Rust dep to expose into non-Rust hosts.

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

Writes `rig.toml` + empty `rig.lock`. Detects host language from markers (`Cargo.toml`, `build.zig`, `*.nimble`, …).

## ADD

```bash
rig add --rust rx4
rig i --cargo rx4
rig add --zig some_lib
```

Aliases: `i`, `install` (short package-manager aliases).

Resolves the package, pins it in `rig.toml` / `rig.lock`, and **auto-exposes** a native in-process surface:

- **Rust host ← cargo crate:** adds the dep to `Cargo.toml` and generates `src/rig_bindings/<pkg>.rs` (re-export + markers).
- **Zig host ← cargo crate:** builds a generic `{crate}_ffi` cdylib façade (markers: `{crate}_abi_version` / `_version` / `_name`), writes `src/rig_bindings/<pkg>_bindings.zig`, and patches `build.zig` with `// rig-expose-begin` link markers.
- **Nim host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.nim` (`importc` + `passL` link hints).
- **C/C++ host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.h` (declarations + `-L/-l` metadata macros).
- **C# host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.cs` (`DllImport` P/Invoke wrappers).
- **D host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.d` (`extern(C)` + `pragma(lib)`).
- **V host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.v` (`#flag` + `fn C.…`).
- **Odin host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.odin` (`foreign import` + `foreign` block).
- **Hare host ← cargo crate:** builds the same façade and writes `src/rig_bindings/<pkg>.ha` (`@symbol` C ABI decls + `-L/-l` hints).
- **C/C++/Zig host ← path/git c|cpp|zig:** compiles sources into `target/rig/<pkg>/lib*_native.{dylib,so}` when feasible (flat `.c/.cpp/.zig` or Makefile `$OUT`); clear error otherwise.
- **Rust host ← foreign lang:** generates an equilibrium-ffi `load` path stub.

Ecosystem flags (mutually exclusive): `--cargo`/`--rust`, `--zig`, `--nim`, `--c`, `--cpp`, `--v`, `--d`, `--odin`, `--hare`, `--csharp`/`--cs`.

## UP

```bash
rig up
rig upgrade rx4
```

## REMOVE

```bash
rig ui rx4
rig rm rx4
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
rig show rx4
rig info --nim jester
rig outdated
```

Registry-backed today: **cargo** (crates.io), **nim** (nimble packages.json), **d** (code.dlang.org), **csharp** (NuGet).
**zig** accepts `path:` / `git+` / `https://` pins (search is hint-only).
**c / cpp / v / odin / hare** are path/git pins only.

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

- [`examples/zig-host-rx4`](examples/zig-host-rx4) — Zig ← `rx4` via thin `rx4_ffi` façade + equilibrium-ffi.
- [`examples/zig-host-multi`](examples/zig-host-multi) — Zig ← **multiple** cargo crates (`rx4` + `sha2`) via the **generic** façade.
- [`examples/c-host-sha2`](examples/c-host-sha2) — C ← `sha2` markers + `sha2_hash_256`/`sha2_hash_512` (`cc` + `-lsha2_ffi`).
- [`examples/c-host-libm`](examples/c-host-libm) — C ← **auto-wrapped** `libm` (`libm_sqrt`, …) beyond markers.

```bash
cd examples/zig-host-rx4
rig init
rig add --rust rx4
zig build run

cd ../zig-host-multi
rig sync
zig build run
```

## C ABI façade (cargo → non-Rust hosts)

For every `rig add --rust <crate>` on a Zig (or other non-Rust) host, rig generates a thin `cdylib` under `.rig/shims/<crate>/` (unless the crate already ships `crate-type = ["cdylib"]`, in which case that library is linked through):

| Symbol | Meaning |
|--------|---------|
| `{crate}_abi_version` | Façade ABI revision (`u32`) |
| `{crate}_version` | Null-terminated version string |
| `{crate}_name` | Null-terminated crate name |

Known enrichments (today: `rx4`, `sha2`) may export additional **methods** beyond the markers (e.g. `rx4_prompt_smoke`, `sha2_hash_256`).

For other crates, rig **auto-wraps** a scanned public surface when sources are available (cargo registry / path / crates.io fetch):

- Existing `#[no_mangle] extern "C"` functions are re-exported under the same name.
- Plain `pub fn` / `pub const fn` with only FFI-safe scalar/pointer types become `{crate}_{fn}` exports (façade ABI 2).
- Host binders (Zig/Nim/C/C#/D) emit the discovered declarations; see `.rig/shims/<crate>/surface.json`.

### Honest limits

- **Skipped:** generics, `async`, `impl` methods/traits, tuples/arrays/refs, `str`/`String`/`Vec`, `f16`/`f128`, and other non-FFI-safe types.
- **cbindgen** runs only when the crate ships `cbindgen.toml` **and** the `cbindgen` binary is on `PATH` (provenance header only; scan remains canonical).
- Crates with `#![forbid(unsafe_code)]` still work: unsafe lives only in the generated façade.
- Auto-wrap caps at 256 symbols per crate; enrichments remain intentional for trait-heavy APIs (e.g. `sha2_hash_256`).
- Rust hosts keep the native Cargo re-export path (`src/rig_bindings/<pkg>.rs`); the cdylib façade is for cross-language hosts.

## Manifest

See committed example [`rig.toml`](rig.toml). Schema:

- `[host].language` — rust | zig | nim | c | cpp | v | d | odin | hare | csharp
- `[dependencies.<name>]` — ecosystem, version, git/path, features, expose
- `[expose].dir` — default `src/rig_bindings` (use `src/vendor` for Zig/C imports)

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
- [rotary / rx4](https://github.com/tschk/rotary)

## License

ISC
