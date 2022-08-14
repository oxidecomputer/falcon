#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01GACKYEYP5F53TR3MKFACTYEN/GSwIYsEvEMhqmnNuuMbWym4aLuYcDD8CzHz9T5Lq31y9qyWQ/01GACKYQK9GN885X2T0ED2B1QX/01GACMTE9AMPDGACP3WN39D6Y6/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
