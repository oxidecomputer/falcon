#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01FWMJQ6BRR5MNNGG7G1VEN4M7/JzcHPSuTaByYFjJRJaiXSowwfOKaSVqrqGAOwwHcH1KOwbug/01FWMJQF4P5FBE48BFE773CXW7/01FWMKBQVV8F6EESAPWDV5W77Y/propolis-server

chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
