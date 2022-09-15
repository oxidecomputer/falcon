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
# To cello
#

/opt/oxide/opte/bin/opteadm set-v2p \
	10.0.0.3 \
	A8:40:25:ff:00:03 \
	fd00:3000::1 \
	10

/opt/oxide/opte/bin/opteadm add-router-entry-ipv4 \
	-p xde0 \
	10.0.0.3/32 \
	ip4=10.0.0.3
