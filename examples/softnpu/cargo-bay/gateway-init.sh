#!/bin/bash
ipadm create-addr -t -T static -a 10.100.0.1/24 vioif0/v4

echo "map vioif1 10.100.0.0/24 -> 0/32 portmap tcp/udp auto" > /etc/ipf/ipnat.conf
echo "map vioif1 10.100.0.0/24 -> 0/32" >> /etc/ipf/ipnat.conf

pfexec ipf -E
routeadm -e ipv4-forwarding -u
svcadm enable -s ipfilter

pfexec ipnat -f /etc/ipf/ipnat.conf

dladm set-linkprop -p mtu=1600 vioif0

arp -s 10.100.0.5 a8:e1:de:01:70:1f

ipadm create-addr -T dhcp vioif1/v4
