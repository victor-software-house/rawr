#!/usr/bin/env bash
#MISE description="Each advertised feature set compiles alone"
#MISE env={CARGO_TARGET_DIR = "target/features"}
# verify depends on check:features. A starter: the template writes this file
# once and never rewrites it, so the repository keeps its own feature sets
# here. --workspace covers every member of a workspace and the one package of
# a single crate.
set -euo pipefail
cargo check --locked --workspace --all-targets --no-default-features
