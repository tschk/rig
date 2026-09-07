#!/usr/bin/env bash
# Smoke: path/git C deps (flat sources + CMake) without exotic toolchains.
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

# Host C project
mkdir -p src vendor/flatlib vendor/cmlibel
cat > Makefile <<'EOF'
all:
	@echo ok
EOF
cat > src/main.c <<'EOF'
int main(void) { return 0; }
EOF

# Flat C vendor
cat > vendor/flatlib/flatlib.h <<'EOF'
#pragma once
int flatlib_add(int a, int b);
EOF
cat > vendor/flatlib/flatlib.c <<'EOF'
#include "flatlib.h"
int flatlib_add(int a, int b) { return a + b; }
EOF

# CMake vendor
cat > vendor/cmlibel/CMakeLists.txt <<'EOF'
cmake_minimum_required(VERSION 3.16)
project(cmlibel C)
add_library(cmlibel SHARED cmlibel.c)
set_target_properties(cmlibel PROPERTIES OUTPUT_NAME "cmlibel_native")
EOF
cat > vendor/cmlibel/cmlibel.h <<'EOF'
#pragma once
int cmlibel_mul(int a, int b);
EOF
cat > vendor/cmlibel/cmlibel.c <<'EOF'
#include "cmlibel.h"
int cmlibel_mul(int a, int b) { return a * b; }
EOF

"$RIG" init
"$RIG" add --c "path:$WORK/vendor/flatlib"
"$RIG" add --c "path:$WORK/vendor/cmlibel"

test -f src/rig_bindings/flatlib.h
test -f src/rig_bindings/cmlibel.h
ls target/rig/flatlib/*flatlib_native* >/dev/null
ls target/rig/cmlibel/*cmlibel_native* >/dev/null

echo "ci-smoke-path-c: OK (flat + cmake)"
