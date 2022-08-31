#!/bin/bash

chmod +x softnpuadm

./softnpuadm add-address fe80::aae1:deff:fe01:701c
./softnpuadm add-address fe80::aae1:deff:fe01:701d
./softnpuadm add-address fe80::aae1:deff:fe01:701e

./softnpuadm add-route fd00:1000:: 24 1 fe80::aae1:deff:fe00:1
./softnpuadm add-route fd00:2000:: 24 2 fe80::aae1:deff:fe00:2
./softnpuadm add-route fd00:3000:: 24 3 fe80::aae1:deff:fe00:3

./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:1 a8:e1:de:00:00:01
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:2 a8:e1:de:00:00:02
./softnpuadm add-ndp-entry fe80::aae1:deff:fe00:3 a8:e1:de:00:00:03
