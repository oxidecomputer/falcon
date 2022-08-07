#!/bin/bash

chmod +x routeradm

./routeradm add-address fe80::aae1:deff:fe01:701c
./routeradm add-address fe80::aae1:deff:fe01:701d
./routeradm add-address fe80::aae1:deff:fe01:701e

./routeradm add-route fd00:1000:: 24 1
./routeradm add-route fd00:2000:: 24 2
./routeradm add-route fd00:3000:: 24 3
