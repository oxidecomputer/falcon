#!/bin/bash
ipadm create-addr -t -T static -a 10.100.0.1/24 vioif0/v4

echo "map vioif1 10.100.0.0/24 -> 0/32 portmap tcp/udp 1025:65000" > /etc/ipf/ipnat.conf
echo "map vioif1 10.100.0.0/24 -> 0/32 portmap" >> /etc/ipf/ipnat.conf

pfexec ipf -E
routeadm -e ipv4-forwarding -u
svcadm enable -s ipfilter

pfexec ipnat -f /etc/ipf/ipnat.conf
