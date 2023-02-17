#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01GSF17F77NAEHA3C168HGX7Y2/SAqcSrBk1e8myW5zB7qv7sxcksBHhZqF7rYFwDO5gbNok6N8/01GSF17Q60TKDZ7AY6ZVVKQGBJ/01GSF273KX3GBQMZKR8PQF6ZF0/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
