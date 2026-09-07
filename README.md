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
- **Zig host ← cargo crate:** writes `src/rig_bindings/<pkg>_bindings.zig` (or `[expose].dir`) and patches `build.zig` with `// rig-expose-begin` markers for linking.
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

## Zig host demo

See [`examples/zig-host-rx4`](examples/zig-host-rx4) — Zig host that exposes Rust `rx4` via equilibrium-ffi.

```bash
cd examples/zig-host-rx4
rig init
rig add --rust rx4
zig build
```

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
