#!/bin/bash

set -x

chmod +x softnpuadm

./softnpuadm add-address6 fe80::aae1:deff:fe01:701c
./softnpuadm add-address6 fe80::aae1:deff:fe01:701d
./softnpuadm add-address6 fe80::aae1:deff:fe01:701e
./softnpuadm add-address6 fd00:99::1

./softnpuadm add-route6 fd00:1000:: 24 1 fe80::aae1:deff:fe00:1
./softnpuadm add-route6 fd00:2000:: 24 2 fe80::aae1:deff:fe00:2
./softnpuadm add-route6 fd00:3000:: 24 3 fe80::aae1:deff:fe00:3
./softnpuadm add-route4 0.0.0.0 0 4 10.100.0.1

./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:1 a8:e1:de:00:00:01
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:2 a8:e1:de:00:00:02
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:3 a8:e1:de:00:00:03
./softnpuadm add-arp-entry 10.100.0.1 a8:e1:de:00:00:04

#piano
./softnpuadm add-nat4 10.100.0.5 4000 7000 fd00:2000::1 10 A8:40:25:ff:00:01
#violin
./softnpuadm add-nat4 10.100.0.5 1000 2000 fd00:1000::1 10 A8:40:25:ff:00:02
#cello
./softnpuadm add-nat4 10.100.0.5 2001 3000 fd00:3000::1 10 A8:40:25:ff:00:03
