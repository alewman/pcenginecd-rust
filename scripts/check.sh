#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Formatting, clippy (-D warnings) and the library built for wasm32-unknown-unknown, with the
# rustup 1.93.1 toolchain in target directories of its own (the system cargo's artefacts in
# target/ are another compiler's).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TOOLCHAIN=$(ls -d "$HOME"/.rustup/toolchains/1.93.1-*/bin | head -1)
export PATH="$TOOLCHAIN:$PATH"
cd "$ROOT"
cargo fmt --check
CARGO_TARGET_DIR="$ROOT/target/clippy" nice cargo clippy --release --all-targets -- -D warnings
CARGO_TARGET_DIR="$ROOT/target/wasm" nice cargo build --release --lib --target wasm32-unknown-unknown
ls -la "$ROOT/target/wasm/wasm32-unknown-unknown/release/libpcenginecd_rust.rlib"
echo "check: PASS"
