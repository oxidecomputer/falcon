#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01G5J04PA2HXWMPFM0XKQHV0Q4/j6JtstnpNbsdh2LA3sfjcvC7RQBEktw4FTleVe0gF6FaPM6M/01G5J04Y2P3X8JHQ9BJ4DQTR12/01G5J0YA93SB3V7RZYH0JW60FA/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
