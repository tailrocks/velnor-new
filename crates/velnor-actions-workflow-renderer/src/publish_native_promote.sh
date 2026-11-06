# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -eu
: "${GH_TOKEN:?}" "${VELNOR_IMAGE_SOURCE_SHA:?}" "${VELNOR_TRUSTED_ACTIONS:?}" "${IMAGE_ID:?}"
base="$RUNNER_TEMP/velnor-native-image-promote-$IMAGE_ID"
[ ! -e "$base" ]
umask 077
mkdir -m 700 "$base"
export DOCKER_CONFIG="$base"
registry_auth=$(printf '%s:%s' "$GITHUB_ACTOR" "$GH_TOKEN" | base64 | tr -d '\n')
printf '{"auths":{"ghcr.io":{"auth":"%s"}}}\n' "$registry_auth" > "$DOCKER_CONFIG/config.json"
unset registry_auth
chmod 600 "$DOCKER_CONFIG/config.json"
"$VELNOR_TRUSTED_ACTIONS" image-maintenance promote-image --image-id "$IMAGE_ID" --source-commit "$VELNOR_IMAGE_SOURCE_SHA"
