#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01FX23JYFGRT6WWEBWM7AQ6P8S/FVHQObKPFHxbcKX9myal6ipg31Wozuv4VBU5pRHalj5T6tsU/01FX23K7SZQANV8116B3HVTA2J/01FX247YHE5AART13CNZN888QM/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
