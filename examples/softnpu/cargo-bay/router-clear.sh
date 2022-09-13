#!/bin/bash

set -x

chmod +x softnpuadm

./softnpuadm remove-address6 fe80::aae1:deff:fe01:701c
./softnpuadm remove-address6 fe80::aae1:deff:fe01:701d
./softnpuadm remove-address6 fe80::aae1:deff:fe01:701e
./softnpuadm remove-address6 fd00:99::1

./softnpuadm remove-route6 fd00:1000:: 24
./softnpuadm remove-route6 fd00:2000:: 24
./softnpuadm remove-route6 fd00:3000:: 24
./softnpuadm remove-route4 0.0.0.0 0

./softnpuadm remove-ndp-entry fe80::aae1:deff:fe00:1
./softnpuadm remove-ndp-entry fe80::aae1:deff:fe00:2
./softnpuadm remove-ndp-entry fe80::aae1:deff:fe00:3
./softnpuadm remove-arp-entry 10.100.0.1

./softnpuadm remove-nat4 10.100.0.5 4000 7000
