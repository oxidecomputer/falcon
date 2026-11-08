#!/bin/bash
#
# Install falcon-img as a system service (SMF on illumos, systemd on Linux).
#
#   cargo build --release -p falcon-img
#   pfexec ./falcon-img/pkg/install.sh target/release/falcon-img   # illumos
#   sudo ./falcon-img/pkg/install.sh target/release/falcon-img     # Linux

set -o errexit
set -o pipefail
set -o nounset

if [[ $# -ne 1 ]]; then
    echo "usage: install.sh <path to falcon-img binary>"
    exit 1
fi

bin=$1
pkg=$(cd "$(dirname "$0")" && pwd)
dir=/var/falcon-img

install -d /opt/falcon-img/bin "$dir"
install -m 0755 "$bin" /opt/falcon-img/bin/falcon-img

case "$(uname -s)" in
SunOS)
    svccfg import "$pkg/smf/falcon-img.xml"
    svcadm enable -s svc:/oxide/falcon-img:default
    svcs -l svc:/oxide/falcon-img:default
    ;;
Linux)
    install -m 0644 "$pkg/systemd/falcon-img.service" \
        /etc/systemd/system/falcon-img.service
    systemctl daemon-reload
    systemctl enable --now falcon-img.service
    systemctl --no-pager status falcon-img.service
    ;;
*)
    echo "unsupported OS; run '/opt/falcon-img/bin/falcon-img serve' directly"
    exit 1
    ;;
esac

echo
echo "falcon-img is serving from $dir on port 7780."
echo "Point the DNS name 'falcon-img' at this host for falcon to discover it."
