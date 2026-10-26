// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::yaml::Yaml;

use super::{MANIFEST_SIGNER, SDK_REPOSITORY};

pub(super) fn bootstrap_script() -> String {
    r#"set -euo pipefail
[[ "$VELNOR_HOMEBREW_ADOPTION_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ "$VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$VELNOR_HOMEBREW_ADOPTION_MANIFEST_SHA256" =~ ^[0-9a-f]{64}$ ]]
readonly root="$RUNNER_TEMP/velnor/bootstrap"
mkdir -p "$root" "$RUNNER_TEMP/velnor/bin"
readonly manifest="$root/release-manifest.json"
eval "$VELNOR_ADOPTION_GH" release download "v$VELNOR_HOMEBREW_ADOPTION_VERSION" \
  --repo __SDK_REPOSITORY__ --pattern release-manifest.json --dir "$root"
printf '%s  %s\n' "$VELNOR_HOMEBREW_ADOPTION_MANIFEST_SHA256" "$manifest" | sha256sum --check --status
eval "$VELNOR_ADOPTION_GH" attestation verify "$manifest" \
  --repo __SDK_REPOSITORY__ \
  --signer-workflow __MANIFEST_SIGNER__ \
  --signer-digest "$VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA" \
  --source-ref refs/heads/main \
  --source-digest "$VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA"
/usr/bin/jq -e --arg version "$VELNOR_HOMEBREW_ADOPTION_VERSION" \
  --arg source "$VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA" \
  --arg repository "__SDK_REPOSITORY__" \
  '.schema == 1 and .repository == $repository and .version == $version and .commit == $source' \
  "$manifest" >/dev/null
asset_sha="$(/usr/bin/jq -er --arg version "$VELNOR_HOMEBREW_ADOPTION_VERSION" --arg repository "__SDK_REPOSITORY__" '[.targets[] | select(.target == "x86_64-unknown-linux-gnu" and .artifact == ("https://github.com/" + $repository + "/releases/download/v" + $version + "/velnor-actions-" + $version + "-x86_64-unknown-linux-gnu"))] | if length == 1 then .[0].sha256 else error("target inventory") end' "$manifest")"
readonly asset_sha
[[ "$asset_sha" =~ ^[0-9a-f]{64}$ ]]
readonly asset="$root/velnor-actions-${VELNOR_HOMEBREW_ADOPTION_VERSION}-x86_64-unknown-linux-gnu"
eval "$VELNOR_ADOPTION_GH" release download "v$VELNOR_HOMEBREW_ADOPTION_VERSION" \
  --repo __SDK_REPOSITORY__ --pattern "${asset##*/}" --dir "$root"
printf '%s  %s\n' "$asset_sha" "$asset" | sha256sum --check --status
install -m 0755 "$asset" "$RUNNER_TEMP/velnor/bin/velnor-actions-${VELNOR_HOMEBREW_ADOPTION_VERSION}"
"$RUNNER_TEMP/velnor/bin/velnor-actions-${VELNOR_HOMEBREW_ADOPTION_VERSION}" --version
"#
    .replace("__MANIFEST_SIGNER__", MANIFEST_SIGNER)
    .replace("__SDK_REPOSITORY__", SDK_REPOSITORY)
}

pub(super) fn direct_run(name: &str, script: &str, env: BTreeMap<String, String>) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if !env.is_empty() {
        fields.push((
            "env".to_owned(),
            Yaml::Map(
                env.into_iter()
                    .map(|(key, value)| (key, Yaml::str(value)))
                    .collect(),
            ),
        ));
    }
    fields.extend([
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script)),
    ]);
    Yaml::Map(fields)
}
