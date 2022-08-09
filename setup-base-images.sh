#!/bin/bash

set -e

# Images follow the naming scheme
#   <name>-<os_version>-<image_version>
#
if [[ ! -f ~/.netrc ]]; then
    echo "You must setup a .netrc file with an api.github.com machine entry"
    echo "with a GitHub access token in order to access all needed artifacts"
    exit 1
fi

# Old method of pulling statically defined images
# TODO create new build pipelines for the following images
images="debian-11.0_0 helios-1.1_0"
mkdir -p .img
pushd .img

for img in $images; do
    file=$img.raw.xz
    if [[ ! -f $file ]]; then
        echo "Pulling $file"
        curl -OL https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/$file
    fi
    if [[ ! -f $img.raw ]]; then
        echo "Extracting $file"
        unxz --keep -T 0 $file
    fi
    file=$img.raw

    name=${img%_*}
    if [[ ! -b /dev/zvol/dsk/rpool/falcon/img/$name ]]; then
        echo "Creating ZFS volume $name"
        pfexec zfs create -p -V 20G rpool/falcon/img/$name
        echo "Copying contents of image into volume"
        pfexec dd if=$img.raw of=/dev/zvol/dsk/rpool/falcon/img/$name conv=sync 
        echo "Creating base image snapshot"
        pfexec zfs snapshot rpool/falcon/img/$name@base
    fi
done

function get_current_branch {
    BRANCH=$(curl -n -sSf "https://api.github.com/repos/$REPO" |
        jq -r .default_branch)
    SHA=$(curl -n -sSf "https://api.github.com/repos/$REPO/branches/$BRANCH" |
        jq -r .commit.sha)

    echo "commit $SHA is the head of branch $BRANCH from $REPO"
}

function fetch_and_verify {
    set +e
    sha256sum --status -c "$1.sha256"
    status=$?
    set -e
    if [ $status -eq 0 ]; then
        echo "latest $1 archive present"
    else
        echo "latest $1 archive is not present, or it is corrupted"
        echo "fetching latest $1 archive"
        curl -OL $IMAGE_URL
        sha256sum --status -c "$1.sha256"
    fi
}

function extract_and_verify {
    set +e
    sha256sum --status -c "$IMAGE_NAME.sha256"
    status=$?
    set -e
    if [ $status -eq 0 ]; then
        echo "image already extracted"
    else
        echo "extracting image"
        unxz -T 0 -c -vv $IMAGE_NAME.xz > $VERSION.raw
        sha256sum --status -c "$IMAGE_NAME.sha256"
    fi
}


# New method of discovering and pulling latest image
# Currently only netstack uses this method
IMAGE_NAME=netstack

# TODO put this in a loop / function once we've converted the other
# image repositories
banner $IMAGE_NAME
REPO="oxidecomputer/falcon-image-$IMAGE_NAME"
get_current_branch

# set URLs
ARTIFACT_URL="https://buildomat.eng.oxide.computer/public/file"
IMAGE_BASE_URL="$ARTIFACT_URL/$REPO/image/$SHA"
IMAGE_URL="$IMAGE_BASE_URL/$IMAGE_NAME.xz"
RAW_SHA_URL="$IMAGE_BASE_URL/$IMAGE_NAME.sha256"
IMAGE_SHA_URL="$IMAGE_BASE_URL/$IMAGE_NAME.xz.sha256"

# get image version
VERSION=$(curl -L "$IMAGE_BASE_URL/version.txt")
echo "latest $IMAGE_NAME is $VERSION"

# fetch checksums
curl -L $IMAGE_SHA_URL | sed 's/\/out\///' > $IMAGE_NAME.xz.sha256
curl -L $RAW_SHA_URL | sed "s/\/out\/netstack/$VERSION.raw/" > $IMAGE_NAME.sha256

# If image doesn't exist, fetch image
fetch_and_verify $IMAGE_NAME.xz
extract_and_verify $IMAGE_NAME

name=${VERSION%_*}
if [[ ! -b /dev/zvol/dsk/rpool/falcon/img/$name ]]; then
    echo "Creating ZFS volume $name"
    pfexec zfs create -p -V 20G rpool/falcon/img/$name
    echo "Copying contents of image into volume"
    pfexec dd if=$VERSION.raw of=/dev/zvol/dsk/rpool/falcon/img/$name conv=sync
    echo "Creating base image snapshot"
    pfexec zfs snapshot rpool/falcon/img/$name@base
else
    echo "volume already created for $name"
fi
