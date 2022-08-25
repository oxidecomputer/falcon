#!/bin/bash

set -e
set -o xtrace

dataset=${FALCON_DATASET:-rpool/falcon}
echo "dataset is: $dataset"

# Images follow the naming scheme
#   <name>-<os_version>-<image_version>
#
# Old method of pulling statically defined images
# TODO create new build pipelines for the following images
images="debian-11.0_0 helios-1.1_0"
mkdir -p .img
pushd .img

for img in $images; do
    file=$img.raw.xz
    if [[ ! -f $file ]]; then
        echo "Pulling $file"
        echo "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/$file"
        curl -OL https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/$file
    fi
    if [[ ! -f $img.raw ]]; then
        echo "Extracting $file"
        unxz --keep -T 0 $file
    fi
    file=$img.raw

    name=${img%_*}
    if [[ ! -b /dev/zvol/dsk/$dataset/img/$name ]]; then
        echo "Creating ZFS volume $name"
        pfexec zfs create -p -V 20G "$dataset/img/$name"
        echo "Copying contents of image into volume"
        pfexec dd if=$img.raw of="/dev/zvol/dsk/$dataset/img/$name" conv=sync
        echo "Creating base image snapshot"
        pfexec zfs snapshot "$dataset/img/$name@base"
    fi
done

popd

./get-netstack.sh
