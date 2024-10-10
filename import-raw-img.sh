#!/bin/bash

set -e
set -x

dataset=${FALCON_DATASET:-rpool/falcon}

if [[ $# -ne 2 ]]; then
    echo "usage: import-raw-img <file> <name>"
    exit 1
fi

file=$1
name=$2

if [[ $FORCE == 1 ]]; then
    echo "Deleting $name image"
    pfexec zfs destroy -r $dataset/img/$name || true
fi
if [[ ! -b /dev/zvol/dsk/$dataset/img/$name ]]; then
    echo "Creating ZFS volume $name"
    fsize=`stat --format "%s" $file`
    (( vsize = fsize + 4096 - ( fsize % 4096 ) ))
    pfexec zfs create -p -V $vsize -o volblocksize=4k "$dataset/img/$name"
    echo "Copying contents of image into volume"
    pfexec dd if=$file of="/dev/zvol/rdsk/$dataset/img/$name" bs=1024k status=progress
    echo "Creating base image snapshot"
    pfexec zfs snapshot "$dataset/img/$name@base"
fi
