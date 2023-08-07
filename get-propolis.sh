#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01H6ZF04VHZ3FPXWVKZ08JNE32/Hhe40QjvN0RczlpuS4WQLbbYcerlnJC2yvwMzKqKnuGLjFSK/01H6ZF0FGJN6KCF7GYT19W4J6H/01H6ZFRVBK0E4D54ZSQ5AGT4WD/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
