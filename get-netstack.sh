#!/bin/bash

set -e
set -o xtrace

mkdir -p .img
pushd .img

dataset=${FALCON_DATASET:-rpool/falcon}
echo "dataset is: $dataset"

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
        echo "fetching latest $1 archive from $IMAGE_URL"
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
if [[ -b /dev/zvol/dsk/$dataset/img/$name ]]; then
    echo "volume already created for $name"
    if [[ "$FORCE" == "1" ]]; then
        pfexec zfs destroy -r "$dataset/img/$name"
    else
        exit 0;
    fi
fi

echo "Creating ZFS volume $name"
pfexec zfs create -p -V 20G "$dataset/img/$name"
echo "Copying contents of image $VERSION into volume"
pfexec dd if=$VERSION.raw of="/dev/zvol/dsk/$dataset/img/$name" conv=sync
echo "Creating base image snapshot"
pfexec zfs snapshot "$dataset/img/$name@base"
