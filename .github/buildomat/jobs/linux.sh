#!/bin/bash
#:
#: name = "linux"
#: variety = "basic"
#: target = "ubuntu-22.04"
#: rust_toolchain = "stable"
#: output_rules = [
#:   "/work/release/*",
#: ]
#:
#: [[publish]]
#: series = "linux"
#: name = "falcon-img"
#: from_output = "/work/release/falcon-img"
#:
#: [[publish]]
#: series = "linux"
#: name = "falcon-img.sha256.txt"
#: from_output = "/work/release/falcon-img.sha256.txt"
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

function digest {
    sha256sum "$1" | awk '{print $1}'
}

banner "build"
cargo build --release -p falcon-img

banner "test"
cargo test -p falcon-img

mkdir -p /work/release
cp target/release/falcon-img /work/release/falcon-img
digest /work/release/falcon-img > /work/release/falcon-img.sha256.txt
