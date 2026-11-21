#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

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
