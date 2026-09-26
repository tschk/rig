# ts-host-crc32fast

TypeScript host calling a cargo crate (`crc32fast`) through [scriptc](https://github.com/vercel-labs/scriptc) `--ffi` bindings, wired by rig.

`rig add` writes `src/rig_bindings/crc32fast.ts` (the declarations) and merges its manifest into `src/rig_bindings/rig.ffi.json`; `rig build` runs `scriptc build` with that manifest and the native library rig built.

```bash
cargo install --path ../..   # or: cargo install rigpkg
npm install -g scriptc       # https://github.com/vercel-labs/scriptc
cd examples/ts-host-crc32fast
rig init                     # tsconfig.json / package.json -> host: typescript
rig add --rust crc32fast -y  # declarations + merged --ffi manifest
rig build -- src/main.ts -o host
./host
```

Expected:

```text
ts-host-crc32fast: crc32fast abi= 2 crc32(hello)= 907060870
```

Notes:

- scriptc consumer mode binds scalar returns/parameters and `const uint8_t *` + `size_t` spans (`Uint8Array`); `const char *` returns such as `crc32fast_version` have no outbound class and are skipped with a warning naming the reason.
- The built host records each native library by absolute path, so run `rig sync` / `rig build` again after moving the checkout.
