#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01HBEK1VP0XXQ5AGHQWG4VHHKC/fhGuUKZ1HmugHzQGV0IQIHDlOxLHCMAN9A5GfwxxywEiL6bc/01HBEK24DF8XE8XGTR2BGDPVXD/01HBEKT35XJM7PMK471A2FV2KM/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
