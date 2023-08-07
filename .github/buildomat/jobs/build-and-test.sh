#!/bin/bash
#:
#: name = "build-and-test"
#: variety = "basic"
#: target = "helios-2.0"
#: rust_toolchain = "stable"
#: output_rules = []
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

banner "build"
ptime -m cargo build --all
ptime -m cargo build --all --release

banner "check"
cargo fmt --all -- --check
cargo clippy --all -- --deny warnings

#
# TODO 
# tbe following will not work unless we run on a bare metal instance, which is
# expensive.
#

# check to see if we have virsatualization extensions
#pfexec isainfo -v
#pfexec isainfo -x

#isainfo -x | egrep "(svm|vmx)"

#banner "setup"
#./get-ovmf.sh
#./setup-base-images.sh
#./get-propolis.sh

#export RUST_BACKTRACE=1
#export RUST_LOG=trace

#banner "test"
#pfexec cargo test -- --test-threads 1
#pfexec cargo test -- --test-threads 1 --ignored
