#!/bin/bash

curl -OL  https://buildomat.eng.oxide.computer/wg/0/artefact/01J4QN90YQBKK0B095P1KESX47/V4c9mEBv79HtOsmXeCz4C1m4dQBDdzB2ESHAxvA3UdZIdG4O/01J4QN9CV8X92EBANRYWVMNZR2/01J4QNXRHZQBSJZFXQDE2Y2JJB/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
