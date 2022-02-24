#!/bin/bash
#:
#: name = "build-and-test"
#: variety = "basic"
#: target = "helios"
#: rust_toolchain = "nightly-2021-11-24" 
#: output_rules = []
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

banner "build"
ptime -m cargo build
ptime -m cargo build --release

banner check
#cargo fmt -- --check
#cargo clippy

banner "image setup"
./get-ovmf.sh
./setup-base-images.sh

export RUST_BACKTRACE=1
export RUST_LOG=trace

banner "test"
pfexec cargo test -- --test-threads 1
pfexec cargo test -- --test-threads 1 --ignored
