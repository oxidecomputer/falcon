#!/bin/bash

# get propolis version from Cargo.toml
rev=`cat Cargo.toml | grep propolis | sed -E 's/.*rev = "([^"]+)".*/\1/'`
echo "Fetching propolis version $rev"

curl -OL https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/$rev/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
