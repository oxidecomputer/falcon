#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01J47SP5WG406KHSV43K8TBVVA/Mm53OBbS1Seynk2zC2N6BFMdCRUVzr8t66s78CGOIbFBpqFa/01J47SPHH2ST71530E4YT9EP3Z/01J47TB8PB1FG0A299WCNP1B04/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
