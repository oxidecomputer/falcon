#!/bin/bash

set -x
set -e

## opte =======================================================================

name=xde0

instance_ip=10.0.0.2
instance_mac=A8:40:25:ff:00:01

gateway_mac=A8:40:25:00:00:01
gateway_ip=10.0.0.254

boundary_services_addr=fd00:99::1
boundary_services_vni=99

vpc_vni=10
vpc_subnet=10.0.0.0/24
source_underlay_addr=fd00:2000::1

snat_start=4000
snat_end=7000
snat_ip=10.100.0.5
snat_gw_mac=a8:e1:de:00:00:04

/opt/oxide/opte/bin/opteadm create-xde \
    $name \
    --private-mac $instance_mac \
    --private-ip $instance_ip \
    --gateway-mac $gateway_mac \
    --gateway-ip $gateway_ip \
    --bsvc-addr $boundary_services_addr \
    --bsvc-vni $boundary_services_vni \
    --vpc-vni $vpc_vni \
    --vpc-subnet $vpc_subnet \
    --src-underlay-addr $source_underlay_addr \
    --snat-start $snat_start \
    --snat-end $snat_end \
    --snat-ip $snat_ip \
    --snat-gw-mac $snat_gw_mac

/opt/oxide/opte/bin/opteadm add-router-entry-ipv4 -p xde0 '0.0.0.0/0' ig

## vnic =======================================================================

set -x


dladm create-vnic -t -l xde0 -m $instance_mac vnic0
