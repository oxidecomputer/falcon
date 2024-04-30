#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01HWNQE2GN1HF0FZ9TB3FW45EA/gQ0fn20VcNgnMykIfnhD6GdXx4eeiEny46kZTY9pHLJvcluk/01HWNQEEMJNB86YGC83Z0FG2WE/01HWNR2ARSN1QJG1NGP9S60XCN/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
