#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01J6W7HNVVWNXZKE0GFDC3MDG3/d6LnQXh0v9uk19YRRZzTG9ILNJD32nQe51QgzSQKCDdDZ2za/01J6W7J5HNW26WFAV7M6KYD43F/01J6W87ECGKJGAXDQFPYZ7JMJ4/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
