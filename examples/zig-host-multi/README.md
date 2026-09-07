# zig-host-multi

Zig host demo: call **multiple** cargo crates (`rx4` + `sha2`) via rig’s generic C ABI façades.

```bash
cargo install rigpkg   # or: cargo install --path ../..
cd examples/zig-host-multi
rig sync               # builds `.rig/shims/{rx4,sha2}` + patches build.zig
zig build run
```

Expected output (versions vary):

```text
zig-host-multi: rx4 ABI=2 version=0.7.1
zig-host-multi: sha2 ABI=1 version=0.10.9
```

## Symbols

| Crate | Lib | Symbols |
|-------|-----|---------|
| `rx4` | `rx4_ffi` | `rx4_abi_version`, `rx4_version`, `rx4_name`, plus enrichment (`rx4_agent_*`, `rx4_prompt_smoke`) |
| `sha2` | `sha2_ffi` | `sha2_abi_version`, `sha2_version`, `sha2_name` (generic markers) |

`sha2` has no C API of its own; the façade proves **any** `rig add --rust <crate>` gets a linkable cdylib with discoverable markers. Full Rust API auto-export is out of scope (see root README).
