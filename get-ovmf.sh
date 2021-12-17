#!/bin/bash

curl -OL https://dev.goodwu.net/cargo-bay-2/OVMF_CODE.fd
pfexec mkdir -p /var/ovmf
pfexec mv OVMF_CODE.fd /var/ovmf/OVMF_CODE.fd
