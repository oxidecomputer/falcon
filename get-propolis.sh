#!/bin/bash

curl -OL https://buildomat.eng.oxide.computer/wg/0/artefact/01GKAGZWTYQMNZVZYQNH99N5F2/14N8NTF7oj4b25oLM2oSGyGj102Ciqo4VV7kcminJPmCQHir/01GKAH052JACY454V95JGC6W8G/01GKAJ08FE9KVX4G64259ENPQT/propolis-server
chmod +x propolis-server
pfexec mv propolis-server /usr/bin/
