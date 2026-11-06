# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

validate_journal() {
  jq -ser --arg sha "$sha" --arg profile "$profile" --arg repository "$repository" \
    --argjson ci "$ci_id" --argjson attempt "$ci_attempt" --slurpfile candidate "$2" \
    'select(length == 2 and .[0].status == "pending") | .[1] |
     select(.schema == "velnor.image-publication.v1" and .status == "verified" and
       .source_commit == $sha and .source_ref == "refs/heads/main" and .profile == $profile and
       .repository == $repository and .tag == ("sha-" + $sha) and
       .workflow_path == ".github/workflows/publish.yml" and .workflow_authority_sha == $sha and
       .producer_run_id == $ci and .producer_run_attempt == $attempt and
       .ci_run_id == $ci and .ci_run_attempt == $attempt and
       .registry_digest == .platform_manifest_digest and
       .platform_manifest_digest == $candidate[0].oci.platform_manifest_digest and
       .image_configuration_digest == $candidate[0].oci.image_configuration_digest) |
       .registry_digest | select(test("^sha256:[0-9a-f]{64}$"))' "$1"
}

validate_publisher_jobs() {
  jq -e --arg name "Publish native image ($role)" --arg sha "$sha" --argjson run "$publish_id" --argjson attempt "$publish_attempt" \
    '[.[].jobs[] | select(.name == $name)] | length == 1 and .[0].run_id == $run and .[0].run_attempt == $attempt and .[0].head_sha == $sha and .[0].status == "completed" and .[0].conclusion == "success"' "$1" >/dev/null

}

validate_digest_attestation() {
  jq -e --arg sha "$sha" --arg repository "$repository" --arg digest "${digest#sha256:}" \
    --arg invocation "https://github.com/$repo/actions/runs/$publish_id/attempts/$publish_attempt" \
    '[.[].verificationResult | select(.signature.certificate.runInvocationURI == $invocation and
       .signature.certificate.sourceRepositoryIdentifier == "1390620900" and
       .signature.certificate.sourceRepositoryDigest == $sha and .signature.certificate.buildSignerDigest == $sha and
       .signature.certificate.runnerEnvironment == "github-hosted" and .signature.certificate.buildTrigger == "push" and
       .statement.predicateType == "https://slsa.dev/provenance/v1" and
       (.statement.subject | length) == 1 and .statement.subject[0].name == $repository and
       .statement.subject[0].digest.sha256 == $digest)] | length == 1' "$1" >/dev/null

}

fetch_publisher_jobs() {
expected_total=
: > "$base/publisher-jobs.jsonl"
for ((page=1;page<=20;page++)); do
  (ulimit -f 32768; gh api "repos/$repo/actions/runs/$publish_id/attempts/$publish_attempt/jobs?per_page=100&page=$page" > "$base/jobs-page.json")
  total=$(jq -er '.total_count | select(type == "number" and . == floor and . >= 0 and . <= 2000)' "$base/jobs-page.json")
  if [[ -z "$expected_total" ]]; then expected_total=$total; else [[ "$total" == "$expected_total" ]]; fi
  count=$(jq -er '.jobs | length | select(. <= 100)' "$base/jobs-page.json")
  jq -c '.jobs[]' "$base/jobs-page.json" >> "$base/publisher-jobs.jsonl"
  [[ "$count" == 100 ]] || break
done
[[ "$page" -le 20 ]]
jq -se --argjson total "$expected_total" 'length == $total and all(.[]; .id | type == "number" and . == floor and . > 0) and ([.[].id] | unique | length) == $total' "$base/publisher-jobs.jsonl" >/dev/null
jq -s '[{jobs:.}]' "$base/publisher-jobs.jsonl" > "$base/jobs.json"
}
