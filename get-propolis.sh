#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01HFAFE96NCPZJNBF6PW3PP986/yitxU6XK2dA309oP5UrNkFOpgDZeFK3IiooWQgF8oCdV8qbY/01HFAFET8B2SFM7NDB4K53AFAT/01HFAG8SG6266CSKHMJ8W676YK/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
