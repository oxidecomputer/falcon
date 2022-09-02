#!/bin/bash

set -x
set -e

mkdir -p /tmp/piano
chmod +x piano/scripts/*.sh
cp -r piano/scripts /tmp/piano/

mkdir -p /instance-test-zones
zfs create -p -o mountpoint=/instance-test-zones rpool/instance-test-zones

pkg set-publisher --search-first helios-dev

zonecfg -z iz1 -f piano/zone1.txt
zoneadm -z iz1 install
zoneadm -z iz1 boot

pkg set-publisher --search-first helios-netdev

# wait for zone to be ready
sleep 3

zlogin iz1 /scripts/netup.sh
