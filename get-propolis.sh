#!/bin/bash

# from propolis' `falcon` job, commit ba4d338f0c98faa0328502c99aa6fa0e0948d3da
curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01JQA00H3HQ2H7JW0PWMDQ7XDN/52rIztYQlpRJEttYOMaflSu2b8GnmLPUnW6TsjkChj37dvgp/01JQA00TDFWX2870391FPKBY76/01JQA0N556K971D0XGQPGJYVKJ/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
