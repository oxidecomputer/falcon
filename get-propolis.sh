#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01G5VRQHKC8N69EF8QSF7WGZRG/5FuWF1eb3AbHObLRuOFFiDkr1iFAlWG9xDvSqWC4GC3nw8bi/01G5VRQV7XKZ7BJYPA3WSYEM6V/01G5VSHEGS0THJAH16NPTFV8R2/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
