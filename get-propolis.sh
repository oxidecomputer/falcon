#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01HE32FGWQ0PVSJN8A4R7MD06C/Mah1dCoejq19niYXlNIsmhds9lSKf62PyuO8SG9NP5SQQPRG/01HE32FS8F8W1ZAEPV8AKZ5JWW/01HE336NXE027YQC4380PY6WWR/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
