# c-host-libm

C host demo: `rig add --rust libm` auto-wraps FFI-safe `pub fn`s (e.g. `libm_sqrt`) from the `libm` crate.

```bash
rig init
rig add --rust libm@0.2.16 -y
make run
```
