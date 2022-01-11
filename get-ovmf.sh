#!/bin/bash

set -e

mkdir -p .img
pushd .img

if [[ ! -f OVMF_CODE.fd ]]; then
    echo "Pulling OVMF_CODE.fd"
    curl -OL https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/OVMF_CODE.fd
fi

echo "Copying OVMF to /var/ovmf"
pfexec mkdir -p /var/ovmf
pfexec cp OVMF_CODE.fd /var/ovmf/OVMF_CODE.fd
