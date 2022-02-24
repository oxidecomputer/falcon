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

# check to see if we have virsatualization extensions
pfexec isainfo -v
pfexec isainfo -x

isainfo -x | egrep "(svm|vmx)"
