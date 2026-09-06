# zig-host-rx4

Minimal Zig host demo: native in-process call into `rx4` via rig + equilibrium-ffi.

```bash
cd examples/zig-host-rx4
cargo install --path ../..   # or: cargo install --git https://github.com/tschk/rig
rig init
rig add --rust rx4
zig build run
```

Expected output (version varies):

```text
zig-host-rx4: native rx4 ABI=1 version=0.7.1
```

`rig add` builds a thin `rx4_ffi` cdylib façade (crates.io `rx4` has no `cdylib`),
writes `src/rig_bindings/rx4_bindings.zig` (equilibrium-ffi imports + portable externs),
and patches `build.zig` to link `target/rig/rx4`.

Symbols: `rx4_abi_version`, `rx4_version`.
