#!/bin/bash
# testcontainers 0.27 uses /.dockerenv to decide it is inside a container,
# then get_host() returns the bridge gateway. This worker shares the DinD
# network namespace, so published ports are on localhost. An IP host makes
# OpenSSL verify an IP SAN and reject a localhost certificate.
set -euo pipefail
sudo -n rm -f -- /.dockerenv
