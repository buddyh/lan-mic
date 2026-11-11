#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
# Strip local checkout/registry paths from panic locations and other embedded text.
separator=$'\037'
export CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS:+$CARGO_ENCODED_RUSTFLAGS$separator}--remap-path-prefix=$HOME=/build${separator}--remap-path-prefix=$root=/source"
cd "$root"
cargo build --locked --release "$@"
