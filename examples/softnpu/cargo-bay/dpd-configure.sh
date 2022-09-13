#!/bin/bash

./swadm dp port set 1:0 ipv6 fe80::aae1:deff:fe01:701c
./swadm dp port set 2:0 ipv6 fe80::aae1:deff:fe01:701d
./swadm dp port set 3:0 ipv6 fe80::aae1:deff:fe01:701e

#./swadm dp route add fd00:1000::/24 1:0 fe80::aae1:deff:fe00:1
#./swadm dp route add fd00:2000::/24 2:0 fe80::aae1:deff:fe00:2 
#./swadm dp route add fd00:3000::/24 3:0 fe80::aae1:deff:fe00:3

#./swadm dp arp add -i fe80::aae1:deff:fe00:1 -m a8:e1:de:0:0:1
#./swadm dp arp add -i fe80::aae1:deff:fe00:2 -m a8:e1:de:0:0:2
#./swadm dp arp add -i fe80::aae1:deff:fe00:3 -m a8:e1:de:0:0:3
