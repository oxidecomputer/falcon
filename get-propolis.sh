#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01GBPKTP9Z2S98VN5J8PQK3V3J/Od78nXJ73d8K0LCUnsenvEZT6ezITMBJ1NlFEMVAgPaeopZU/01GBPKTYBXCHNKM5ZFJXSCZTEY/01GBPMXNGVXY31FH0MKZD9GHTH/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
