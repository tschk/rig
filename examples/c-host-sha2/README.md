# c-host-sha2

C host calling a cargo crate (`sha2`) via rig’s generic `{crate}_ffi` façade.

```bash
cargo install --path ../..   # or: cargo install rigpkg
cd examples/c-host-sha2
rig init --host c            # if needed; or edit rig.toml
# Prefer: copy committed rig.toml below, then:
rig sync
cc -o main src/main.c -I src/rig_bindings -I target/rig/sha2 \
  -L target/rig/sha2 -lsha2_ffi -Wl,-rpath,target/rig/sha2
./main
```

Expected:

```text
c-host-sha2: sha2 ABI=1 version=0.10.9 name=sha2
```
