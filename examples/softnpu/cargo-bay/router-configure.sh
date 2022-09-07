#!/bin/bash

chmod +x softnpuadm

./softnpuadm add-address6 fe80::aae1:deff:fe01:701c
sleep 0.25
./softnpuadm add-address6 fe80::aae1:deff:fe01:701d
sleep 0.25
./softnpuadm add-address6 fe80::aae1:deff:fe01:701e
sleep 0.25
./softnpuadm add-address6 fe00:99::1
sleep 0.25

./softnpuadm add-route6 fd00:1000:: 24 1 fe80::aae1:deff:fe00:1
sleep 0.25
./softnpuadm add-route6 fd00:2000:: 24 2 fe80::aae1:deff:fe00:2
sleep 0.25
./softnpuadm add-route6 fd00:3000:: 24 3 fe80::aae1:deff:fe00:3
sleep 0.25
./softnpuadm add-route4 10.100.0.0  24 4 10.100.0.5
sleep 0.25

./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:1 a8:e1:de:00:00:01
sleep 0.25
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:2 a8:e1:de:00:00:02
sleep 0.25
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:3 a8:e1:de:00:00:03
sleep 0.25
./softnpuadm add-arp-entry 10.100.0.5 a8:e1:de:00:00:04
sleep 0.25

./softnpuadm add-nat4 10.100.0.5 4000 7000 fd00:2000::1 
