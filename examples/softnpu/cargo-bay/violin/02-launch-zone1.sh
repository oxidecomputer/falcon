#!/bin/bash

set -x
set -e

mkdir -p /tmp/violin
chmod +x violin/scripts/*.sh
cp -r violin/scripts /tmp/violin/

mkdir -p /instance-test-zones
zfs create -p -o mountpoint=/instance-test-zones rpool/instance-test-zones

pkg set-publisher --search-first helios-dev

zonecfg -z iz1 -f violin/zone1.txt
zoneadm -z iz1 install
zoneadm -z iz1 boot

pkg set-publisher --search-first helios-netdev

# wait for zone to be ready
sleep 3

zlogin iz1 /scripts/netup.sh
