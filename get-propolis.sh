#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01HWT9E3HTD7M58MD1VJP8ZZ09/a5obopISYhQaVlQ4PX8dfGWIowWs51drDMzVkTK2DBoo2D0I/01HWT9EP3DVZGG1RW0QYTXS5DY/01HWTA6K1RQWGH45ECTGFJJ3E1/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
