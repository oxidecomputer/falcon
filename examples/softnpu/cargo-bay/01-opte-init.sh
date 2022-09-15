#!/bin/bash

set -x

dladm create-etherstub -t stub0

/opt/oxide/opte/bin/opteadm set-xde-underlay vioif0 stub0
