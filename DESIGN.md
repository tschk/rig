# rig — Design

Cross-language native dependency manager for polyglot hosts.
CLI-first: detect host language, resolve packages across ecosystems, track them in
`rig.toml`, and **auto-expose** them in-process via [equilibrium-ffi](https://github.com/tschk/equilibrium).

**License: ISC** (match equilibrium). `Cargo.toml` must set `license = "ISC"`.
Copyright line: `Copyright (c) 2026 The Software Company of Hong Kong & Contributors`.

**Vibe:** wax/oil package-manager ergonomics (`i`/`up`/`ui`/`dr`/`ls`/`s`) applied to
*project-local native deps*, not system formulae. Manifest = `rig.toml`. Lockfile =
`rig.lock`. Every mutate path (`add`/`rm`/`up`/`sync`) keeps equilibrium-ffi expose
artifacts in sync.

**Refs:** equilibrium · rotary/rx4 · telekinesis (README clarity bar) · wax (Homebrew-compat PM on PATH) · oil (“oil up” generation/lock vibe).

---

## 1. CLI command surface + clap structure

### 1.1 Public surface (must ship)

| Command | Aliases | Purpose |
|---------|---------|---------|
| `rig init` | — | Create `rig.toml` (+ optional lock) in cwd; detect host lang |
| `rig add <pkgs>…` | `i`, `install` | Resolve + pin deps; update manifest/lock; re-expose |
| `rig remove <pkgs>…` | `rm`, `ui`, `uninstall` | Drop deps; rewrite expose; refresh lock |
| `rig upgrade [pkgs]…` | `up` | Bump to latest (or named); rewrite expose; refresh lock |
| `rig list [query]` | `ls` | List deps from `rig.toml` / lock (filter optional) |
| `rig search <query>` | `s`, `find` | Search ecosystem registries (honors `--cargo` etc.) |
| `rig info <pkg>` | `show` | Show resolved metadata (version, source, expose status) |
| `rig outdated` | — | List deps with newer versions available |
| `rig lock` | — | Regenerate `rig.lock` from `rig.toml` + resolved graph |
| `rig sync` | — | Install/expose exactly what `rig.lock` says |
| `rig check` | `doctor`, `dr` | Validate toolchains, manifest, expose freshness |
| `rig build` | — | Build host project + ensure expose artifacts are current |

Global flags (wax-shaped): `-v/--verbose`, `-y/--yes`, `--dry-run`, `--ask`,
`--time-to-action`, `-h/--help`, `-V/--version`.

### 1.2 Ecosystem selectors (on `add` / `i` / `search` / `info` / `upgrade`)

Exactly one ecosystem flag, or auto from host detection / package heuristic:

| Flag | Alias intent | Registry |
|------|--------------|----------|
| `--cargo` | `--rust` | crates.io (+ git/path) |
| `--zig` | — | Zig package manager / URL / path |
| `--nim` | — | Nimble |
| `--c` / `--cpp` | — | path/git/vcpkg-ish source pin (no central “crates”) |
| `--v` | — | VPM / path / git |
| `--d` | — | DUB |
| `--odin` | — | Odin collection / path / git |
| `--hare` | — | path / git (Hare has no crates.io) |
| `--csharp` | — | NuGet |

`--cargo` and `--rust` are **aliases for the same ecosystem** (product copy uses both;
clap: one enum variant, two long flags via `alias` / dual `Arg::long`).

Example (hero path):

```bash
rig add --rust rx4
# ≡
rig i --cargo rx4
```

### 1.3 Clap module layout

```text
src/
  main.rs                 # Cli::parse(); dispatch; exit codes
  cli/
    mod.rs                # pub struct Cli { #[command(subcommand)] cmd, globals }
    globals.rs            # Verbose/Yes/DryRun/Ask/TimeToAction
    ecosystem.rs          # EcosystemFlag enum + clap group
    commands/
      mod.rs
      init.rs
      add.rs              # aliases: install, i  — Args { pkgs, ecosystem, features, … }
      remove.rs           # aliases: rm, ui, uninstall
      upgrade.rs          # aliases: up  — optional pkgs; --self later
      list.rs             # aliases: ls
      search.rs           # aliases: s, find
      info.rs             # aliases: show
      outdated.rs
      lock.rs
      sync.rs
      check.rs            # aliases: doctor, dr  — --fix, --full
      build.rs
```

Clap sketch (canonical names + aliases):

```rust
#[derive(Parser)]
#[command(name = "rig", version, about = "Cross-lang native deps via equilibrium-ffi")]
pub struct Cli {
    #[command(flatten)]
    pub globals: Globals,
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create rig.toml in the current project
    Init(InitArgs),

    /// Add packages and auto-expose via equilibrium-ffi
    #[command(visible_aliases = ["i", "install"])]
    Add(AddArgs),

    /// Remove packages and refresh expose
    #[command(visible_aliases = ["rm", "ui", "uninstall"])]
    Remove(RemoveArgs),

    /// Upgrade packages (all if omitted) — oil/wax "up" vibe
    #[command(visible_aliases = ["up"])]
    Upgrade(UpgradeArgs),

    #[command(visible_aliases = ["ls"])]
    List(ListArgs),

    #[command(visible_aliases = ["s", "find"])]
    Search(SearchArgs),

    #[command(visible_aliases = ["show"])]
    Info(InfoArgs),

    Outdated(OutdatedArgs),
    Lock(LockArgs),
    Sync(SyncArgs),

    /// Validate toolchains + expose freshness
    #[command(visible_aliases = ["doctor", "dr"])]
    Check(CheckArgs),

    Build(BuildArgs),
}

#[derive(Args, Clone)]
pub struct EcosystemArgs {
    /// crates.io / Cargo (alias: --rust)
    #[arg(long = "cargo", visible_alias = "rust", group = "eco")]
    pub cargo: bool,
    #[arg(long, group = "eco")] pub zig: bool,
    #[arg(long, group = "eco")] pub nim: bool,
    #[arg(long, group = "eco")] pub c: bool,
    #[arg(long, group = "eco")] pub cpp: bool,
    #[arg(long = "v", group = "eco")] pub vlang: bool,
    #[arg(long = "d", group = "eco")] pub dlang: bool,
    #[arg(long, group = "eco")] pub odin: bool,
    #[arg(long, group = "eco")] pub hare: bool,
    #[arg(long, group = "eco")] pub csharp: bool,
}
```

Dispatch stays thin: `Command::*` → `commands::*_run(ctx)` with shared `AppCtx`
(`manifest`, `lock`, `host: DetectedHost`, `http`, `eq`).

Exit codes: `0` ok · `1` user/resolve error · `2` doctor failure · `3` expose/build fail.

---

## 2. `rig.toml` schema (concrete example with rx4)

```toml
# rig.toml — project-local native dependency manifest
schema_version = 1

[host]
# Detected or set by `rig init`. Drives default expose consumer language.
language = "zig"          # rust | zig | nim | c | cpp | v | d | odin | hare | csharp
# Optional explicit project roots (override detection)
# manifest = "build.zig"
# root = "."

[package]
name = "fx-fork"          # optional; borrowed from host manifest when present
version = "0.0.0"

# Dependencies keyed by package id as used on the CLI.
# Each entry records ecosystem + resolution + expose intent.

[dependencies.rx4]
ecosystem = "cargo"       # cargo|zig|nim|c|cpp|v|d|odin|hare|csharp
# Requirement (semver for cargo/nuget/dub/nimble; git/path otherwise)
version = "0.x"           # caret-ish requirement string
# Optional precise source overrides:
# git = "https://github.com/tschk/rotary"
# rev = "…"
# path = "../rotary"
features = ["builtin-tools", "providers"]
default_features = false
expose = true             # AUTO-EXPOSE via equilibrium-ffi (default true)

[dependencies.rx4.expose]
# Consumer = host language unless overridden
consumer = "zig"
# Where generated imports land (host-relative)
out = "src/vendor/rx4_bindings.zig"
# Build artifact / cdylib expectations for cargo packages
crate_types = ["cdylib", "rlib"]
# equilibrium-ffi load path after build (relative)
native = "target/rig/rx4"

[dependencies.some_nim_lib]
ecosystem = "nim"
version = "1.2.0"
expose = true

[expose]
# Global defaults for auto-expose
enabled = true
dir = "src/vendor"        # default out dir for generated consumer imports
cache = ".rig/cache"
build_dir = "target/rig"

[tool.rig]
# Reserved for future config (mirrors cargo [package.metadata] vibe)
```

**Lockfile `rig.lock` (sketch):**

```toml
schema_version = 1
[[package]]
name = "rx4"
ecosystem = "cargo"
version = "0.6.17"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "…"
features = ["builtin-tools", "providers"]
expose_consumer = "zig"
expose_out = "src/vendor/rx4_bindings.zig"
```

`rig lock` / `add` / `rm` / `up` / `sync` own this file. Commit it.

---

## 3. Language detection algorithm

Order (first confident hit wins; ties → ask or `--host`):

1. **Explicit** — `rig.toml` `[host].language` if present and valid.
2. **Marker files** (cwd → parents until VCS root / fs root):
   | Marker | Language |
   |--------|----------|
   | `Cargo.toml` | rust |
   | `build.zig` / `build.zig.zon` | zig |
   | `*.nimble` / `nimble.paths` | nim |
   | `dub.json` / `dub.sdl` | d |
   | `*.csproj` / `*.sln` | csharp |
   | `v.mod` | v |
   | `ols.json` / many `*.odin` at root | odin |
   | `*.ha` + Hare layout | hare |
   | `CMakeLists.txt` / `meson.build` / `Makefile` + `.c/.h` dominant | c |
   | same + `.cpp/.hpp` dominant | cpp |
3. **Extension census** — if no marker, count source files (ignore `target/`, `.git`,
   `node_modules`, `.rig/`); majority extension via equilibrium’s map
   (`.rs` `.zig` `.nim` `.c/.h` `.cpp` `.cs` `.v` `.d` `.odin` `.ha`).
4. **Ambiguous** — print candidates; non-interactive → error unless `-y` picks first
   or `RIG_HOST_LANG` set.
5. **`rig init`** writes the winner into `[host].language`.

Detection is shared with equilibrium’s extension table but **project-scoped** (markers
first). Do not shell out to `eq` for detection; reimplement the thin marker table in
`rig::detect`.

---

## 4. How `rig add` resolves packages per ecosystem

Pipeline for `rig add [--eco] <spec>…`:

1. Detect host (unless `--host` / manifest).
2. Resolve ecosystem flag or infer from spec (`crates.io`-looking name + rust host → cargo).
3. **Resolve** each spec → concrete version + source + checksum/metadata.
4. Merge into `rig.toml` `[dependencies.*]`.
5. Refresh `rig.lock`.
6. **Fetch/build** native artifact as needed for expose.
7. **Auto-expose** (section 5).
8. Print telekinesis-style short summary (name, version, expose path).

### Per-ecosystem resolvers (`src/resolve/`)

| Eco | Resolver behavior |
|-----|-------------------|
| **cargo/rust** | crates.io API / sparse index; support `name`, `name@version`, git URL, path. Features flags forwarded. Prefer library crates that can emit `cdylib` (warn if not). |
| **zig** | `build.zig.zon` URL/hash fetch; path/git. |
| **nim** | Nimble registry / git / path. |
| **d** | DUB registry. |
| **csharp** | NuGet flat container. |
| **v** | VPM / git / path. |
| **odin / hare / c / cpp** | Primarily **path** and **git** pins; optional known collection URLs. Version = rev/tag. |

Common `PackageSpec` parse: `name`, `name@version`, `git+https://…`, `path:…`.

`search` / `info` / `outdated` reuse the same resolver clients (read-only).

---

## 5. Auto-expose with equilibrium-ffi

**Goal:** after `rig add --rust rx4` in a **Zig** host (fx-style), the Zig project can
`@import` generated bindings and link the built native lib — no hand-written FFI.

### 5.1 Rust package → non-Rust host (primary path; fx demo)

```
rx4 (crates.io) 
  → cargo build -p rx4 --crate-type cdylib (in .rig/cache or target/rig)
  → produce lib + C header (cbindgen / equilibrium-rust #[ffi] surface / existing header)
  → equilibrium_ffi::generate_imports(header, Language::Zig, …)
  → write src/vendor/rx4_bindings.zig
  → patch host build (build.zig addObjectFile / addLibrary) OR drop a small
    `rig_expose.zig` stub that `@import`s bindings + links path from rig.toml
```

For crates without a clean C ABI:

1. Prefer packages that already export `extern "C"` / use `equilibrium-rust`.
2. Else generate a **thin cdylib shim** crate under `.rig/shims/<pkg>/` that wraps the
   public Rust API behind `#[no_mangle] extern "C"` for a declared surface
   (MVP: require `cdylib` + header; shim generation is post-MVP — see gaps).

### 5.2 Rust host consuming foreign langs

```
foreign.zig/.nim/.v/…
  → equilibrium_ffi::load / compile_to_c
  → Rust bindings via generate_bindings / load() module handle
  → build.rs snippet or `src/vendor/<pkg>.rs` included from lib.rs
```

`rig add --zig foo` inside a Cargo project updates `build.rs` (idempotent markers
`// rig-expose-begin` … `// rig-expose-end`) and `rig.toml`.

### 5.3 Same-lang adds

Still record in `rig.toml` and prefer the **native** package manager when useful
(`cargo add` for rust→rust) **plus** keep expose metadata consistent so cross-lang
hosts cloning the repo get the same graph. For rust→rust, expose step may no-op
beyond manifest (document clearly).

### 5.4 Mutators that re-sync expose

`add` · `remove` · `upgrade` · `sync` · `build` (if stale) · `check --fix`.

Stale detection: hash(manifest + lock + header mtime) vs stamp in `.rig/expose-stamp`.

### 5.5 Other hosts (parity matrix)

| Host \ Dep | cargo | zig | nim | c/cpp | v | d | odin | hare | csharp |
|------------|-------|-----|-----|-------|---|---|------|------|--------|
| rust       | native/`cargo add` | eq load | eq | eq | eq | eq | eq | eq | eq/NuGet→C |
| zig        | **hero** eq imports | zon | eq | eq | … | … | … | … | … |
| others     | eq consumer imports | … | native where exists | … | … | … | … | … | … |

MVP ships **Rust host full** + **Zig host ← cargo dep** (rx4). Other cells: resolve +
manifest + best-effort expose; harden in priority order (section 9).

---

## 6. Module layout (Rust crate)

```text
rig/                          # binary crate (ISC)
  Cargo.toml                  # license = "ISC", name = "rig"
  LICENSE                     # ISC text
  README.md                   # telekinesis-clarity
  DESIGN.md                   # this file
  src/
    main.rs
    lib.rs                    # library surface for tests
    cli/                      # §1.3
    detect/
      mod.rs                  # host language detection
    manifest/
      mod.rs                  # rig.toml serde
      lock.rs                 # rig.lock
      schema.rs
    resolve/
      mod.rs                  # PackageSpec, Resolver trait
      cargo.rs
      zig.rs
      nim.rs
      dub.rs
      nuget.rs
      git_path.rs             # shared git/path for c/cpp/odin/hare/v
    expose/
      mod.rs                  # orchestrate equilibrium-ffi
      stamp.rs
      rust_host.rs
      zig_host.rs
      consumer.rs             # Language → generate_imports
      build_patch.rs          # idempotent build.zig / build.rs edits
    ops/
      init.rs add.rs remove.rs upgrade.rs
      list.rs search.rs info.rs outdated.rs
      lock.rs sync.rs check.rs build.rs
    util/
      http.rs edit.rs paths.rs
  tests/
    cli_smoke.rs
    detect_fixtures/
    manifest_roundtrip.rs
    expose_zig_rx4.rs         # #[ignore] without network/toolchains
  examples/
    zig-host-rx4/             # fx-fork demo skeleton
  .github/workflows/ci.yml
```

Deps (indicative): `clap` (derive) · `serde`/`toml` · `anyhow`/`thiserror` ·
`reqwest` or `ureq` · `semver` · `ignore`/`walkdir` · `equilibrium-ffi` (git) ·
`crates_io_api` or sparse index client · `sha2` · `tempfile`.

Binary name: `rig`. Library: `rig` (for integration tests).

---

## 7. Test plan + CI matrix

### 7.1 Local / unit

- Manifest + lock roundtrip (incl. rx4 example).
- Detection fixtures for each marker file.
- Clap: every alias parses to the same command (`i`→Add, `ui`→Remove, `up`→Upgrade,
  `dr`→Check, `s`→Search, `ls`→List, `show`→Info).
- Ecosystem flag group exclusivity.
- Expose stamp stale/fresh logic (no compiler).

### 7.2 Integration (feature `tools` / ignored by default)

- `rig init` in temp dirs per host marker.
- `rig add --cargo serde` on a tiny Rust host (no expose needed).
- `rig add --rust rx4` on Zig fixture → bindings file exists + `zig build` (optional).
- `rig remove` / `up` / `lock` / `sync` / `outdated` / `doctor`.

### 7.3 CI matrix (`.github/workflows/ci.yml`)

| Job | OS | Notes |
|-----|----|-------|
| `cargo fmt/clippy/test` | ubuntu-latest | always |
| `cargo test` | macos-latest | always |
| `expose-zig` | ubuntu | install zig; run ignored expose test |
| `expose-rust-foreign` | ubuntu | install one of nim/zig; foreign→rust load |
| windows | windows-latest | cli parse + detect; expose best-effort |

Use equilibrium’s `setup-equilibrium` action when needing compilers.
Cache `.rig/cache` + cargo.

README badges + `rig doctor` documented as the human CI analogue.

---

## 8. fx fork demo plan (`rig add --rust rx4` in Zig host)

**Context:** fx is a **Zig** coding agent (vercel-labs/fx lineage). telekinesis contrasts
with fx on size; this demo shows **rx4 (Rust harness) exposed into a Zig host** via rig —
the cross-lang path, not rust→rust.

### Steps

1. `examples/zig-host-rx4/` (or external `fx` fork worktree) with minimal `build.zig` +
   `src/main.zig` that would call into an agent loop ABI (trimmed: e.g. `rx4_version`
   / one `prompt` C ABI stub if full Agent surface is too large for MVP).
2. From that tree:

   ```bash
   rig init                 # detects zig
   rig add --rust rx4
   # writes rig.toml, rig.lock, src/vendor/rx4_bindings.zig, links in build.zig
   zig build
   zig build run
   ```

3. Document in README under **DEMO** with telekinesis-level brevity:

   ```text
   ADD
   rig add --rust rx4

   EXPOSE
   equilibrium-ffi generates Zig imports; build.zig links the cdylib.
   ```

4. CI job `expose-zig` builds the example when secrets/toolchains allow.

**Honest note:** full `rx4::Agent` is a large Rust API; MVP expose may ship a **narrow
C ABI façade** crate `rx4-ffi` (or `.rig/shims/rx4`) until rotary exports
`equilibrium-rust`/`cdylib` officially. Track as P0 gap if crates.io `rx4` lacks cdylib.

---

## 9. Honest MVP-vs-full gaps (priority order)

| P | Gap | MVP stance |
|---|-----|------------|
| P0 | Full wax surface wired | **Ship all commands** listed in §1 (even if `search`/`outdated` are registry-thin). |
| P0 | `rig add --cargo/--rust` + Zig expose | Must work for demo path; shim if rx4 has no C ABI. |
| P0 | `rig.toml` + `rig.lock` + ISC license | Required. |
| P0 | `init` / `rm` / `up` / `list` / `lock` / `sync` / `check` / `build` | Functional on cargo+zig hosts. |
| P1 | Rust host ← zig/nim/c via `equilibrium_ffi::load` + build.rs patch | Implement after Zig←cargo. |
| P1 | Real crates.io sparse index + checksums in lock | MVP may pin version via API JSON; harden checksums next. |
| P1 | README telekinesis-clarity + fx demo example | Same milestone as P0 expose. |
| P2 | nim/d/nuget/v/odin/hare resolvers | Resolve git/path everywhere; registry clients incremental. |
| P2 | Auto shim generation for pure-Rust APIs | Manual/façade first. |
| P2 | `upgrade --self`, pin/unpin, why/deps (wax extras) | Out of scope until core PM feels good. |
| P3 | Interactive TUI list/search like wax | Print-only is fine. |
| P3 | Generation rollback à la oil | Lockfile + git is enough; no oil-style generations. |

---

## 10. README clarity bar (telekinesis)

Mirror https://telekinesis.tsc.hk tone: short sections, monospace commands, no essay.

Suggested skeleton:

```text
rig
Cross-lang native deps. equilibrium-ffi expose. one manifest.

INSTALL
cargo install rig

INIT
rig init

ADD
rig add --rust rx4
rig i --zig some_lib

UP
rig up

REMOVE
rig ui rx4

CHECK
rig dr

LICENSE
ISC
```

---

## 11. Recommended implementation order

1. Crate skeleton: `Cargo.toml` (`license = "ISC"`), `LICENSE`, clap surface **with all
   aliases** (commands may stub with `todo!` only for registry-heavy ones — prefer thin
   real impls).
2. `detect` + `manifest`/`lock` serde + `rig init`.
3. `resolve/cargo` + `add`/`remove`/`list`/`lock`/`sync` for cargo specs.
4. `expose/zig_host` + equilibrium-ffi generate_imports; stamp; `build`/`check`.
5. `upgrade`/`outdated`/`info`/`search` (cargo).
6. Rust-host foreign load path.
7. Other ecosystem resolvers (git/path first).
8. `examples/zig-host-rx4` + CI matrix + README polish.
9. Façade/shim story for rx4 if needed; then fx fork demo write-up.

---

## 12. Conflicts / coordination notes

- Repo was empty (git only) when this DESIGN was written — **no code conflicts**.
- Other executor owns implementation under `/Users/undivisible/projects/rig`. Prefer
  they consume this file; avoid both writing `src/cli/**` simultaneously.
- If they already chose different alias names, **reconcile to §1** (wax-compat is
  product requirement).
- License must be **ISC**, not MPL (rotary/telekinesis are MPL; equilibrium/rig are ISC).
- Do not invent a second manifest name — **`rig.toml` only**.

### Review notes

*(Append when the tree has substantial code. Checklist for the reviewer.)*

- [ ] Every alias in §1.1 parses
- [ ] `license = "ISC"` + LICENSE file
- [ ] `rig add --rust` ≡ `--cargo`
- [ ] Mutators refresh expose stamp
- [ ] Zig←rx4 demo path documented or `examples/` present
- [ ] No Cursor-cloud-agent leftovers; CI uses setup-equilibrium where needed

---

## 13. Review notes (tree as of write time)

**Tree state:** `Cargo.toml` stub (no `license`, no deps), `src/main.rs` hello-world only,
`.rig-agent/IMPLEMENT.md` present (other executor brief). No clap/manifest code yet — low
merge risk if they read this file before scaffolding `src/cli`.

### Concrete fix list for the other executor

1. **Set `license = "ISC"`** in `Cargo.toml` immediately; add root `LICENSE` (ISC). Stub
   currently omits `license` and `description`/`repository`.
2. **Clap surface:** IMPLEMENT.md matches §1 aliases — good. Prefer modular `src/cli/`
   (§1.3) over a single `cli.rs` once commands grow; either OK for MVP if aliases match.
3. **Manifest schema drift (RESOLVE NOW):**
   - DESIGN §2 uses `[host].language` + keyed `[dependencies.<name>]`.
   - IMPLEMENT.md uses `[project].lang` + `[[libs]]` array.
   - **Decision: prefer DESIGN §2 keyed tables** (cargo-like, stable keys for `add`/`rm`).
     If you already started on `[[libs]]`, either switch before merge or add a v1→v2
     read path — do not ship two conflicting examples.
4. **Copyright line:** equilibrium uses “The Software Company of Hong Kong & Contributors”.
   IMPLEMENT says “Max Carter / tschk”. Either is fine for ISC; pick one and keep
   LICENSE + DESIGN consistent (recommend TSC.hk line to match equilibrium).
5. **`--csharp` / `--cs`:** IMPLEMENT adds `--cs` alias — accept; add to EcosystemArgs.
6. **Hero path clarity:** IMPLEMENT emphasizes Rust host + `rig add --rust rx4` (Cargo.toml
   edit + `src/rig_bindings/`). DESIGN §8 hero demo is **Zig host ← rx4**. Ship **both**:
   Rust-host path is P0 for IMPLEMENT; Zig←cargo remains the fx cross-lang demo (P0/P1).
7. **Expose output path:** DESIGN default `src/vendor/…`; IMPLEMENT `src/rig_bindings/…`.
   Pick one string and document in README — recommend `src/rig_bindings/` if Rust-host
   re-exports are primary; keep `src/vendor/` for Zig/C imports. Allow
   `[expose].dir` override (already in §2).
8. **Edition:** stub has `edition = "2024"` — OK if MSRV matches equilibrium/rx4 (≥1.88).
9. **Do not** block on oil generations / wax `pin`/`why` — out of scope (§9).
10. Next impl order stays §11; start clap stubs + manifest before expose.

### Alignment checklist

- [ ] `license = "ISC"` + LICENSE
- [ ] One manifest schema (DESIGN §2) in code + example `rig.toml`
- [ ] All §1 aliases parse
- [ ] `--cargo` ≡ `--rust`; `--csharp` ≡ `--cs`
- [ ] add/rm/up/sync refresh expose
- [ ] Rust-host rx4 path + Zig-host demo both documented
