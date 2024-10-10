#!/bin/bash

set -o xtrace
set -o errexit
set -o pipefail
set -o nounset

dataset=${FALCON_DATASET:-rpool/falcon}
echo "dataset is: $dataset"

# Images follow the naming scheme
#   <name>-<os_version>-<image_version>
#
# Old method of pulling statically defined images
# TODO create new build pipelines for the following images
images="debian-11.0_0 helios-2.5_0"
mkdir -p .img
pushd .img

for img in $images; do
    name=${img%_*}
    file=$img.raw.xz
    if [[ $FORCE == 1 ]]; then
        rm -f $file
        rm -rf $img.raw
        echo "Deleting $name image"
        pfexec zfs destroy -r $dataset/img/$name || true
    fi

    if [[ -b /dev/zvol/dsk/$dataset/img/$name ]]; then
        continue
    fi

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

    echo "Creating ZFS volume $name"
    fsize=`stat --format "%s" $img.raw`
    (( vsize = fsize + 4096 - ( fsize % 4096 ) ))
    pfexec zfs create -p -V $vsize -o volblocksize=4k "$dataset/img/$name"
    echo "Copying contents of image into volume"
    pfexec dd if=$img.raw of="/dev/zvol/rdsk/$dataset/img/$name" bs=1024k status=progress
    echo "Creating base image snapshot"
    pfexec zfs snapshot "$dataset/img/$name@base"
done

popd
