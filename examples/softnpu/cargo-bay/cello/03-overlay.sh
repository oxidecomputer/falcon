#!/bin/bash

set -x

#
# To violin
#

/opt/oxide/opte/bin/opteadm set-v2p \
	10.0.0.1 \
	A8:40:25:ff:00:02 \
	fd00:1000::1 \
	10

/opt/oxide/opte/bin/opteadm add-router-entry-ipv4 \
	-p xde0 \
	10.0.0.1/32 \
	ip4=10.0.0.1

#
# To piano
#

/opt/oxide/opte/bin/opteadm set-v2p \
	10.0.0.2 \
	A8:40:25:ff:00:01 \
	fd00:2000::1 \
	10

/opt/oxide/opte/bin/opteadm add-router-entry-ipv4 \
	-p xde0 \
	10.0.0.2/32 \
	ip4=10.0.0.2
