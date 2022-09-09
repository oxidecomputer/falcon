#!/bin/bash

ipadm create-addr -t -T addrconf vioif0/l6
ipadm create-addr -t -T addrconf vioif1/l6
ipadm create-addr -t -T addrconf vioif2/l6
ipadm create-addr -t -T static -a 10.100.0.2/24 vioif3/v4

chmod +x /opt/cargo-bay/softnpuadm
chmod +x router-configure.sh
