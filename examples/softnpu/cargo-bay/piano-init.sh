#!/bin/bash

ipadm create-addr -t -T dhcp vioif1/v4
echo "nameserver 8.8.8.8" > /etc/resolv.conf

ipadm create-addr -t -T addrconf vioif0/l6
ipadm create-addr -t -T static -a fd00:2000::1/64 vioif0/v6
route add -inet6 fd00:1000::/24 fe80::aae1:deff:fe01:701d
route add -inet6 fd00:3000::/24 fe80::aae1:deff:fe01:701d
route add -inet6 fd00:99::/32 fe80::aae1:deff:fe01:701d

# warm up ndp
#sleep 1
#ping -ns fe80::aae1:deff:fe01:701d 60 4

dladm set-linkprop -p mtu=1600 vioif0

rem_drv xde
cp /opt/cargo-bay/xde /kernel/drv/amd64/
add_drv xde
chmod +x /opt/cargo-bay/opteadm
cp /opt/cargo-bay/opteadm /opt/oxide/opte/bin/

chmod +x /opt/cargo-bay/*.sh
chmod +x /opt/cargo-bay/piano/*.sh
chmod +x /opt/cargo-bay/piano/scripts/*.sh
