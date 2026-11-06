#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
: "${VELNOR_RELEASE_TASK_REQUEST_FILE:?}"
: "${VELNOR_RELEASE_OUTPUT_DIRECTORY:?}"
cargo run --quiet --locked --release --package velnor-actions-cli --example sdk-preview-producer -- \
  "$VELNOR_RELEASE_TASK_REQUEST_FILE" "$VELNOR_RELEASE_OUTPUT_DIRECTORY"
