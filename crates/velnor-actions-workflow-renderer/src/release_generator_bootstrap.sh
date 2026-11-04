# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
umask 077
: "${RUNNER_TEMP:?}" "${GH_TOKEN:?}" "${VELNOR_GENERATOR_VERSION:?}" "${VELNOR_GENERATOR_SOURCE_SHA:?}" "${VELNOR_GENERATOR_MANIFEST_SHA256:?}"
unset VELNOR_BOOTSTRAP_GH_TOKEN
VELNOR_BOOTSTRAP_GH_TOKEN=$GH_TOKEN
unset GH_TOKEN
version=$VELNOR_GENERATOR_VERSION
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
root=$RUNNER_TEMP/velnor
out=$root/bootstrap
[ ! -e "$root" ]
mkdir -p "$out" "$root/bin"
fetch() { /usr/bin/curl -fsSL --connect-timeout 15 --max-time 120 --max-filesize "$3" --max-redirs 5 --proto '=https' --proto-redir '=https' --tlsv1.2 "$1" -o "$2"; }
fetch https://github.com/jqlang/jq/releases/download/jq-1.8.1/jq-linux-amd64 "$out/jq" 104857600
printf '%s  %s\n' 020468de7539ce70ef1bceaf7cde2e8c4f2ca6c3afb84642aabc5c97d9fc2a0d "$out/jq" | /usr/bin/sha256sum -c -
chmod 500 "$out/jq"
jq() { "$out/jq" "$@"; }
gharchive=$out/gh.tar.gz
fetch https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_linux_amd64.tar.gz "$gharchive" 104857600
printf '%s  %s\n' 4c806978ffce1cec70853302a56d70ac6da4f19bae7adca4e1a8ab00633dd91d "$gharchive" | /usr/bin/sha256sum -c -
(ulimit -f 65536; /usr/bin/tar -xzf "$gharchive" -C "$out" --no-same-owner --no-same-permissions gh_2.103.3_linux_amd64/bin/gh)
gh=$out/gh_2.103.3_linux_amd64/bin/gh
[ -f "$gh" ] && [ ! -L "$gh" ]
printf '%s  %s\n' b768184570b12536be4ac1528893f6340d1c6a7b40b56ad45bca9cd0e87ad34d "$gh" | /usr/bin/sha256sum -c -
chmod 500 "$gh"
repo=tailrocks/velnor-new
base=https://github.com/$repo/releases/download/v$version
manifest=$out/release-manifest.json
name=velnor-actions-$version-x86_64-unknown-linux-gnu
fetch "$base/release-manifest.json" "$manifest" 1048576
printf '%s  %s\n' "$VELNOR_GENERATOR_MANIFEST_SHA256" "$manifest" | /usr/bin/sha256sum -c -
fetch "$base/release-manifest.json.intoto.jsonl" "$manifest.intoto.jsonl" 5242880
fetch "$base/$name" "$out/$name" 104857600
fetch "$base/$name.intoto.jsonl" "$out/$name.intoto.jsonl" 5242880
jq -e --arg v "$version" --arg base "$base" --arg name "$name" '.schema == 1 and .repository == "tailrocks/velnor-new" and .version == $v and (.commit | test("^[0-9a-f]{40}$")) and ([.targets[] | select(.target == "x86_64-unknown-linux-gnu")] | length == 1) and ([.targets[] | select(.target == "x86_64-unknown-linux-gnu")][0] | .artifact == ($base + "/" + $name) and (.sha256 | test("^[0-9a-f]{64}$")))' "$manifest" >/dev/null
commit=$(jq -r .commit "$manifest")
[ "$commit" = "$VELNOR_GENERATOR_SOURCE_SHA" ]
digest=$(jq -r '.targets[] | select(.target == "x86_64-unknown-linux-gnu") | .sha256' "$manifest")
printf '%s  %s\n' "$digest" "$out/$name" | /usr/bin/sha256sum -c -
verify() {
 (ulimit -f 32768; GH_TOKEN="$VELNOR_BOOTSTRAP_GH_TOKEN" "$gh" attestation verify "$1" --bundle "$1.intoto.jsonl" --repo "$repo" --signer-workflow "$repo/.github/workflows/product-release-generator.yml" --source-ref refs/heads/main --source-digest "$commit" --signer-digest "$commit" --deny-self-hosted-runners --format json) > "$2"
 jq -e --arg sha "$commit" --arg file "$(basename "$1")" --arg digest "$(/usr/bin/sha256sum "$1" | cut -d ' ' -f 1)" '
 length == 1 and (.[0].verificationResult as $r |
 $r.statement as $s | $r.signature.certificate as $c |
 $s.predicate.buildDefinition as $b | $s.predicate.runDetails as $run |
 $s.predicateType == "https://slsa.dev/provenance/v1" and $b.buildType == "https://actions.github.io/buildtypes/workflow/v1" and
 $b.externalParameters.workflow == {path:".github/workflows/product-release.yml", ref:"refs/heads/main", repository:"https://github.com/tailrocks/velnor-new"} and
 $b.internalParameters.github.event_name == "workflow_dispatch" and $b.internalParameters.github.repository_id == "1390620900" and $b.internalParameters.github.repository_owner_id == "78806509" and $b.internalParameters.github.runner_environment == "github-hosted" and
 $b.resolvedDependencies == [{uri:"git+https://github.com/tailrocks/velnor-new@refs/heads/main",digest:{gitCommit:$sha}}] and
 (if $file == "release-manifest.json" then $s.subject == [{name:$file,digest:{sha256:$digest}}] else (($s.subject | length) == 3 and (all($s.subject[]; (.digest | keys == ["sha256"] and (.sha256 | test("^[0-9a-f]{64}$"))))) and ([$s.subject[].name] | sort) == ([$file,($file + ".sha256"),($file + ".provenance.json")] | sort) and ([ $s.subject[] | select(.name == $file and .digest == {sha256:$digest}) ] | length == 1)) end) and
 $run.builder.id == "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-generator.yml@refs/heads/main" and
 $c.issuer == "https://token.actions.githubusercontent.com" and $c.subjectAlternativeName == $run.builder.id and $c.buildSignerURI == $run.builder.id and $c.buildSignerDigest == $sha and
 $c.sourceRepositoryURI == "https://github.com/tailrocks/velnor-new" and $c.sourceRepositoryOwnerURI == "https://github.com/tailrocks" and $c.sourceRepositoryIdentifier == "1390620900" and $c.sourceRepositoryOwnerIdentifier == "78806509" and $c.sourceRepositoryDigest == $sha and $c.sourceRepositoryRef == "refs/heads/main" and
 $c.buildConfigURI == "https://github.com/tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main" and $c.buildConfigDigest == $sha and $c.buildTrigger == "workflow_dispatch" and $c.runnerEnvironment == "github-hosted" and
 $c.runInvocationURI == $run.metadata.invocationId and ($c.runInvocationURI | test("^https://github.com/tailrocks/velnor-new/actions/runs/[1-9][0-9]*/attempts/[1-9][0-9]*$")))' "$2" >/dev/null
}
verify "$manifest" "$out/manifest-verified.json"
verify "$out/$name" "$out/binary-verified.json"
[ "$(jq -r '.[0].verificationResult.signature.certificate.runInvocationURI' "$out/manifest-verified.json")" = "$(jq -r '.[0].verificationResult.signature.certificate.runInvocationURI' "$out/binary-verified.json")" ]
cp "$out/$name" "$root/bin/velnor-actions-$version"
chmod 500 "$root/bin/velnor-actions-$version"
