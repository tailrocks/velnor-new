// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::ImagePlatform;

pub const ASSEMBLE_STEP_ID: &str = "image";
const DOCKER_CONFIG_DIR: &str = "$RUNNER_TEMP/velnor/docker-config";

#[must_use]
pub fn crane_function(prefix: &[String]) -> String {
    format!("crane() {{ {} \"$@\"; }}", prefix.join(" "))
}

const GET: &str = "get() { \"$@\" > \"$dir/.val\"; IFS=' ' read -r val _ < \"$dir/.val\"; }";

fn work_dir(id: &str) -> String {
    format!("$RUNNER_TEMP/velnor/image/{id}")
}

fn archs(platforms: &[ImagePlatform]) -> String {
    platforms
        .iter()
        .map(|platform| platform.job_suffix())
        .collect::<Vec<_>>()
        .join(" ")
}

#[must_use]
pub fn owner_script(owner: &str) -> String {
    format!(
        "set -eu; printf '%s\\n' \"$GITHUB_REPOSITORY_OWNER\" | tr 'A-Z' 'a-z' | {{ read -r owner; [ '{owner}' = \"$owner\" ]; }}"
    )
}

#[must_use]
pub fn download_script(gh: &str, id: &str, branch: &str, platforms: &[ImagePlatform]) -> String {
    let dir = work_dir(id);
    let archs = archs(platforms);
    let delays = crate::schema2::RETRY_DELAYS_SECONDS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let empty_program = concat!(
        r#"if type != "array" then error("ci_run_listing_invalid") "#,
        r#"elif length == 0 then error("ci_run_listing_invalid") "#,
        r#"elif any(.[]; type != "object") then error("ci_run_listing_invalid") "#,
        r#"elif any(.[]; (.workflow_runs | type) != "array") then error("ci_run_listing_invalid") "#,
        r#"else ([.[] | .workflow_runs[]] | length == 0) end"#,
    );
    let program = format!(
        "[.[] | .workflow_runs[] | select(.head_sha == env.GITHUB_SHA and .event == \"push\" and .conclusion == \"success\" and .head_branch == \"{branch}\" and .head_repository.full_name == env.GITHUB_REPOSITORY)] | sort_by(.id) | last | .id // error(\"ci_run_not_found\")"
    );
    format!(
        "set -eu; {gh}; {GET}; dir=\"{dir}\"; rm -rf \"$dir\"; mkdir -p \"$dir/in\"; \
         for delay in {delays} ''; do \
         gh api --paginate --slurp \"repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$GITHUB_SHA&per_page=100\" > \"$dir/.listing\"; \
         get jq -r '{empty_program}' \"$dir/.listing\"; \
         if [ \"$val\" = false ]; then break; fi; \
         if [ -z \"$delay\" ]; then break; fi; \
         sleep \"$delay\"; done; \
         get jq -er '{program}' \"$dir/.listing\"; run=$val; \
         printf '%s\\n' \"$run\" | grep -Eqx '[0-9]+'; \
         for arch in {archs}; do gh run download \"$run\" --repo \"$GITHUB_REPOSITORY\" \
         --name \"velnor-image-{id}-$arch\" --dir \"$dir/in/$arch\"; done;"
    )
}

#[must_use]
pub fn assemble_script(id: &str, platforms: &[ImagePlatform]) -> String {
    let dir = work_dir(id);
    let archs = archs(platforms);
    format!(
        "set -eu; dir=\"{dir}\"; {GET}; rm -rf \"$dir/lay\" \"$dir/merged\" \"$dir/sbom\"; \
         mkdir -p \"$dir/lay\" \"$dir/sbom\" \"$dir/merged/blobs/sha256\"; tops=''; \
         for arch in {archs}; do {PER_PLATFORM} done; {MERGE}"
    )
}

const PER_PLATFORM: &str = "\
in=\"$dir/in/$arch\"; lay=\"$dir/lay/$arch\"; \
ls -A \"$in\" | sort | tr '\\n' ' ' | grep -Fqx 'image.tar image.tar.sha256 metadata.json '; \
(cd \"$in\" && sha256sum -c --status -- image.tar.sha256); \
if tar -tf \"$in/image.tar\" | grep -Eq '(^/|(^|/)\\.\\.(/|$))'; then exit 1; fi; \
if tar -tvf \"$in/image.tar\" | cut -c1 | grep -q '^[^-d]'; then exit 1; fi; \
mkdir -p \"$lay\"; tar -xf \"$in/image.tar\" -C \"$lay\" --no-same-owner --no-same-permissions; \
get jq -er '.manifests | if length == 1 and .[0].mediaType == \"application/vnd.oci.image.index.v1+json\" then .[0].digest else error(\"layout_shape\") end' \"$lay/index.json\"; top=$val; \
get jq -er '.\"containerimage.digest\"' \"$in/metadata.json\"; [ \"$top\" = \"$val\" ]; \
topfile=\"$lay/blobs/sha256/${top#sha256:}\"; \
get sha256sum \"$topfile\"; [ \"$val\" = \"${top#sha256:}\" ]; \
get jq -er --arg a \"$arch\" '[.manifests[] | select(.platform.os == \"linux\" and .platform.architecture == $a and (.annotations[\"vnd.docker.reference.type\"] // \"\") == \"\")] | if length == 1 then .[0].digest else error(\"platform_image_count\") end' \"$topfile\"; img=$val; \
get jq -er --arg d \"$img\" '[.manifests[] | select(.annotations[\"vnd.docker.reference.type\"] == \"attestation-manifest\" and .annotations[\"vnd.docker.reference.digest\"] == $d)] | if length == 1 then .[0].digest else error(\"attestation_manifest_count\") end' \"$topfile\"; att=$val; \
get jq -er '[.layers[] | select(.annotations[\"in-toto.io/predicate-type\"] == \"https://spdx.dev/Document\")] | if length == 1 then .[0].digest else error(\"sbom_layer_count\") end' \"$lay/blobs/sha256/${att#sha256:}\"; layer=$val; \
jq -ce '.predicate | select(.spdxVersion != null and .SPDXID != null)' \"$lay/blobs/sha256/${layer#sha256:}\" > \"$dir/sbom/$arch.spdx.json\"; \
for file in \"$lay/blobs/sha256/\"*; do [ -e \"$dir/merged/blobs/sha256/${file##*/}\" ] || cp -- \"$file\" \"$dir/merged/blobs/sha256/\"; done; \
tops=\"$tops $topfile\"; printf 'digest_%s=%s\\n' \"$arch\" \"$img\" >> \"$GITHUB_OUTPUT\"; ";

const MERGE: &str = "\
jq -c -s '{schemaVersion: 2, mediaType: \"application/vnd.oci.image.index.v1+json\", manifests: (map(.manifests) | add)}' $tops > \"$dir/index.blob\"; \
get sha256sum \"$dir/index.blob\"; idx=$val; get wc -c \"$dir/index.blob\"; size=$val; \
cp -- \"$dir/index.blob\" \"$dir/merged/blobs/sha256/$idx\"; \
printf '{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.index.v1+json\",\"manifests\":[{\"mediaType\":\"application/vnd.oci.image.index.v1+json\",\"digest\":\"sha256:%s\",\"size\":%s}]}\\n' \"$idx\" \"$size\" > \"$dir/merged/index.json\"; \
printf '{\"imageLayoutVersion\":\"1.0.0\"}\\n' > \"$dir/merged/oci-layout\"; \
printf 'digest=sha256:%s\\n' \"$idx\" >> \"$GITHUB_OUTPUT\"";

#[must_use]
pub fn login_script(crane: &str) -> String {
    format!(
        "set -eu; {crane}; cfg=\"{DOCKER_CONFIG_DIR}\"; rm -rf \"$cfg\"; mkdir -p -m 700 \"$cfg\"; \
         printf '%s' \"$GH_TOKEN\" | DOCKER_CONFIG=\"$cfg\" crane auth login ghcr.io --username \"$GITHUB_ACTOR\" --password-stdin; \
         printf 'DOCKER_CONFIG=%s\\n' \"$cfg\" >> \"$GITHUB_ENV\""
    )
}

#[must_use]
pub fn push_script(crane: &str, id: &str, repository: &str, branch_tag: Option<&str>) -> String {
    let dir = work_dir(id);
    let branch = branch_tag.map_or_else(String::new, |tag| {
        format!("; crane tag \"{repository}@$want\" '{tag}'")
    });
    format!(
        "set -eu; {crane}; export DOCKER_CONFIG=\"{DOCKER_CONFIG_DIR}\"; dir=\"{dir}\"; {GET}; \
         get jq -er '.manifests[0].digest' \"$dir/merged/index.json\"; want=$val; ref=\"{repository}:sha-$GITHUB_SHA\"; \
         if crane digest \"$ref\" > \"$dir/.have\" 2> /dev/null; then read -r have < \"$dir/.have\"; [ \"$have\" = \"$want\" ]; fi; \
         crane push \"$dir/merged\" \"$ref\"; get crane digest \"$ref\"; [ \"$val\" = \"$want\" ]{branch}"
    )
}

#[must_use]
pub fn verify_script(gh: &str, id: &str, repository: &str, platforms: &[ImagePlatform]) -> String {
    let dir = work_dir(id);
    let archs = archs(platforms);
    let common = "--repo \"$GITHUB_REPOSITORY\" --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/publish.yml\" --source-digest \"$GITHUB_SHA\"";
    format!(
        "set -eu; {gh}; export DOCKER_CONFIG=\"{DOCKER_CONFIG_DIR}\"; dir=\"{dir}\"; {GET}; \
         get jq -er '.manifests[0].digest' \"$dir/merged/index.json\"; idx=$val; \
         gh attestation verify \"oci://{repository}@$idx\" {common} --predicate-type https://slsa.dev/provenance/v1; \
         for arch in {archs}; do \
         get jq -er --arg a \"$arch\" '[.manifests[] | select(.platform.os == \"linux\" and .platform.architecture == $a and (.annotations[\"vnd.docker.reference.type\"] // \"\") == \"\")][0].digest' \"$dir/merged/blobs/sha256/${{idx#sha256:}}\"; img=$val; \
         get jq -er '.spdxVersion | split(\"-\")[1]' \"$dir/sbom/$arch.spdx.json\"; \
         gh attestation verify \"oci://{repository}@$img\" {common} --predicate-type \"https://spdx.dev/Document/v$val\"; done"
    )
}

#[must_use]
pub fn promote_script(crane: &str, gh: &str, repository: &str, branch: &str) -> String {
    format!(
        "set -eu; {crane}; {gh}; export DOCKER_CONFIG=\"{DOCKER_CONFIG_DIR}\"; \
         dir=\"$RUNNER_TEMP/velnor/image-promote\"; rm -rf \"$dir\"; mkdir -p \"$dir\"; {GET}; \
         ver=${{GITHUB_REF_NAME##*-v}}; get crane digest \"{repository}:sha-$GITHUB_SHA\"; dig=$val; \
         printf '%s\\n' \"$dig\" | grep -Eqx 'sha256:[0-9a-f]{{64}}'; \
         gh attestation verify \"oci://{repository}@$dig\" --repo \"$GITHUB_REPOSITORY\" \
         --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/publish.yml\" \
         --source-digest \"$GITHUB_SHA\" --source-ref refs/heads/{branch} \
         --predicate-type https://slsa.dev/provenance/v1; \
         if crane digest \"{repository}:$ver\" > \"$dir/.have\" 2> /dev/null; then \
         read -r have < \"$dir/.have\"; [ \"$have\" = \"$dig\" ]; \
         else crane tag \"{repository}@$dig\" \"$ver\"; fi; \
         get crane digest \"{repository}:$ver\"; [ \"$val\" = \"$dig\" ]"
    )
}
