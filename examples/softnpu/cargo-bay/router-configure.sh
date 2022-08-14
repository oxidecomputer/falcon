#!/bin/bash

chmod +x routeradm

./routeradm add-address fe80::aae1:deff:fe01:701c
./routeradm add-address fe80::aae1:deff:fe01:701d
./routeradm add-address fe80::aae1:deff:fe01:701e

./routeradm add-route fd00:1000:: 24 1 fe80::8:20ff:feb8:80fc
./routeradm add-route fd00:2000:: 24 2 fe80::8:20ff:fe77:38af
./routeradm add-route fd00:3000:: 24 3 fe80::8:20ff:fe9c:d23d

./routeradm add-ndp-entry fe80::8:20ff:feb8:80fc 02:08:20:b8:80:fc
./routeradm add-ndp-entry fe80::8:20ff:fe77:38af 02:08:20:77:38:af
./routeradm add-ndp-entry fe80::8:20ff:fe9c:d23d 02:08:20:9c:d2:03
