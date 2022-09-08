#!/bin/bash

set -x

chmod +x softnpuadm

./softnpuadm add-address6 fe80::aae1:deff:fe01:701c
sleep 1
./softnpuadm add-address6 fe80::aae1:deff:fe01:701d
sleep 1
./softnpuadm add-address6 fe80::aae1:deff:fe01:701e
sleep 1
./softnpuadm add-address6 fd00:99::1
sleep 1

./softnpuadm add-route6 fd00:1000:: 24 1 fe80::aae1:deff:fe00:1
sleep 1
./softnpuadm add-route6 fd00:2000:: 24 2 fe80::aae1:deff:fe00:2
sleep 1
./softnpuadm add-route6 fd00:3000:: 24 3 fe80::aae1:deff:fe00:3
sleep 1
./softnpuadm add-route4 1.1.1.1  24 4 10.100.0.1
sleep 1

./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:1 a8:e1:de:00:00:01
sleep 1
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:2 a8:e1:de:00:00:02
sleep 1
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:3 a8:e1:de:00:00:03
sleep 1
./softnpuadm add-arp-entry 10.100.0.1 a8:e1:de:00:00:04
sleep 1

./softnpuadm add-nat4 10.100.0.5 4000 7000 fd00:2000::1 
