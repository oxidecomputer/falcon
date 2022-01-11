#!/bin/bash

# get disk
curl -OL https://pkg.oxide.computer/seed/helios-qemu-ttya-full.raw.gz
gunzip helios-qemu-ttya-full.raw.gz

# create volume
pfexec zfs create -p -V 20G rpool/falcon/img/helios

# dump disk into volume
pfexec dd if=helios-qemu-ttya-full.raw of=/dev/zvol/dsk/rpool/falcon/img/helios conv=sync

# create initial snapshot
pfexec zfs snapshot rpool/falcon/img/helios@1.0
