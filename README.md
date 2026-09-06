# rig

Cross-lang native deps. [equilibrium-ffi](https://github.com/tschk/equilibrium) expose. one manifest.

```text
rig add --rust rx4
rig up
rig ui rx4
rig dr
```

Host/embed pattern: [telekinesis](https://telekinesis.tsc.hk) hosts; [rotary](https://github.com/tschk/rotary) (`rx4`) owns the loop.

## Install

```bash
cargo install --git https://github.com/tschk/rig
```

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

Aliases: `i`, `install`.

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
rig show rx4
rig outdated
```

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

## vs fx

[fx](https://github.com/vercel-labs/fx) is a Zig coding agent. `rig` is the **polyglot native dependency manager** that can expose Rust crates like `rx4` into a Zig host (or the reverse) via equilibrium-ffi — project-local, not a system package manager (see [wax](https://github.com/tschk) for Homebrew-compat).

## Links

- [equilibrium](https://github.com/tschk/equilibrium)
- [rotary / rx4](https://github.com/tschk/rotary)
- [telekinesis](https://telekinesis.tsc.hk)
- Design: [`DESIGN.md`](DESIGN.md)

## License

ISC
