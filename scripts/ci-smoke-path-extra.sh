#!/usr/bin/env bash
# Optional extras: Zig path/git when zig is on PATH. Documents Nim/V skips.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RIG="${RIG:-$ROOT/target/release/rig}"
if [[ ! -x "$RIG" ]]; then
  RIG="$ROOT/target/debug/rig"
fi
if [[ ! -x "$RIG" ]]; then
  echo "rig binary not found; run cargo build first" >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "ci-smoke-path-extra: nim/v/odin/hare compile e2e skipped on stock CI (no toolchain install)."
echo "  Local: cargo test --test cli_smoke nim_ e2e / v_ e2e when toolchains present."

if ! command -v zig >/dev/null 2>&1; then
  echo "ci-smoke-path-extra: skip zig path fixture (zig not on PATH)"
  exit 0
fi

mkdir -p src vendor/zmath
cat > build.zig <<'ZIG'
const std = @import("std");
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const exe = b.addExecutable(.{
        .name = "demo",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });
    b.installArtifact(exe);
}
ZIG
cat > src/main.zig <<'ZIG'
pub fn main() void {}
ZIG
cat > vendor/zmath/root.zig <<'ZIG'
export fn zmath_add(a: i32, b: i32) i32 {
    return a + b;
}
ZIG

"$RIG" init
"$RIG" add --zig "path:$WORK/vendor/zmath"
test -f src/rig_bindings/zmath_bindings.zig
ls target/rig/zmath/*zmath_native* >/dev/null

echo "ci-smoke-path-extra: OK (zig path)"
