#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Qualification mode `ports`: two jobs on one host each publish host port 8080
# of their own container, so the runner must give every job its own port
# space. The four `ports-*` jobs of qualification.yml run this script, two on
# GitHub-hosted runners and two on the Velnor scale set.
#
# The script sets no pipefail, so a pipeline's status is the status of its last
# command. `docker port | grep -q` depends on that: `grep -q` exits at the first
# match and `docker port` may then die of SIGPIPE.

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
