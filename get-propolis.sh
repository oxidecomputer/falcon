#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01GS9QP64V1A6ANRAWDJ5CK93S/HRncUwLoTEe2USN7MRiG7V7lXptrmA3OxUT6tPpY5zbk9PKR/01GS9QPJVYZK7JC6655NS1SD93/01GS9RPY6SD3M85RVZ9X7EAT0N/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
