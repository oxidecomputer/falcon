#!/bin/bash
#
# TODO Use DHCP once this issues is resolved.
#
#  https://www.illumos.org/issues/11990

if [[ `zonename` == "global" ]]; then
    echo "This script should be executed in an 'instance' zone";
    exit 1
fi

set -x

# wait for network
while [[ `svcs -Ho STATE network` != "online" ]]; do
    sleep 1
done

instance_addr=10.0.0.1
gateway_addr=10.0.0.254

# add the source address for this instance
ipadm create-addr -t -T static -a $instance_addr/32 vnic0/v4

# add an on-link route to the gateway sourced from the instance address
route add $gateway_addr $instance_addr -interface

# set the default route to go through the gateway
route add default $gateway_addr
