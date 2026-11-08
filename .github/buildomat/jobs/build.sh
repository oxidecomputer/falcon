#!/bin/bash
#:
#: name = "build"
#: variety = "basic"
#: target = "helios-2.0"
#: rust_toolchain = "stable"
#: output_rules = [
#:   "/work/release/*",
#: ]
#:
#: [[publish]]
#: series = "release"
#: name = "falco"
#: from_output = "/work/release/falco"
#:
#: [[publish]]
#: series = "release"
#: name = "falco.sha256.txt"
#: from_output = "/work/release/falco.sha256.txt"
#:
#: [[publish]]
#: series = "release"
#: name = "falcon-img"
#: from_output = "/work/release/falcon-img"
#:
#: [[publish]]
#: series = "release"
#: name = "falcon-img.sha256.txt"
#: from_output = "/work/release/falcon-img.sha256.txt"
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

banner "build"
ptime -m cargo build --release -p falco -p falcon-img

mkdir -p /work/release
for x in falco falcon-img
do
    cp target/release/"${x}" /work/release/"${x}"
    digest -a sha256 /work/release/"${x}" > /work/release/"${x}".sha256.txt
done
