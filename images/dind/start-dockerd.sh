#!/bin/sh

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -eu

if [ "$#" -ne 0 ]; then
	printf >&2 'DinD entrypoint does not accept command overrides\n'
	exit 64
fi

. /usr/local/libexec/velnor/cgroup-v2-delegation.sh
velnor_enable_cgroup_v2
exec /usr/local/bin/dockerd --host=unix:///var/run/docker.sock
