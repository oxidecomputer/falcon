#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01J8TPA802A5SHWPY5K05ZH2J3/ucXqZK2nT2qxxgI0vY5iygOCdEwguBoe9fk3cFrm4IOs9TLX/01J8TPAVRJDHEYKPEG07AHG51H/01J8TQ0QB5M510N99RWD7X55HT/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
