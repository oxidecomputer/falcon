#!/bin/bash

ipadm create-addr -t -T addrconf vioif0/l6
ipadm create-addr -t -T static -a fd00:3000::1/64 vioif0/v6
route add -inet6 fd00:1000::/24 fe80::aae1:deff:fe01:701e
route add -inet6 fd00:2000::/24 fe80::aae1:deff:fe01:701e
route add -inet6 fd00:99::/32 fe80::aae1:deff:fe01:701e

# warm up ndp
#sleep 1
#ping -ns fe80::aae1:deff:fe01:701e 60 4
