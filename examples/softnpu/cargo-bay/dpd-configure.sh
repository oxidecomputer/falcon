#!/bin/bash

./swadm dp port set 1:0 ipv6 fe80::aae1:deff:fe01:701c
sleep 2
./swadm dp port set 2:0 ipv6 fe80::aae1:deff:fe01:701d
sleep 2
./swadm dp port set 3:0 ipv6 fe80::aae1:deff:fe01:701e
sleep 2

./swadm dp route add fd00:1000::/24 1:0
sleep 2
./swadm dp route add fd00:2000::/24 2:0
sleep 2
./swadm dp route add fd00:3000::/24 3:0
