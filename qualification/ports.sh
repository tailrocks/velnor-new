#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Qualification mode `ports`: two jobs on one host each publish host port 8080
# of their own container, so the runner must give every job its own port
# space. This is the script form of the `ports-*` jobs of the old engine qualification.yml,
# which wrote it as one `&&` chain with a `while` loop.
#
# The chain ran under the runner's default shell, `bash -e {0}`, which has no
# pipefail. `docker port | grep -q` must keep it that way: `grep -q` exits at
# the first match and `docker port` may then die of SIGPIPE.

set -eu

image='alpine:3.24.2@sha256:d56c381f961d307a21b3ca004cf1e3910f106644aefb1f43e654c8a56c4fd395'

docker run -d --name g4-hold -p 8080:80 "$image" sleep 120
i=0
while [ "$i" -lt 30 ]; do
  docker port g4-hold 80 | grep -q 8080 && break
  i=$((i + 1))
  sleep 1
done
docker port g4-hold 80 | grep -q 8080
echo port-held
sleep 45
docker rm -f g4-hold
