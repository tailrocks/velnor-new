// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::publish_render::PUBLISH_WORKFLOW_PATH;

#[derive(Debug, Clone, Copy)]
pub struct CosignPin<'a> {
    pub version: &'a str,
    pub sha256: &'a str,
    pub size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CosignPlatform {
    LinuxAmd64,
    LinuxArm64,
}

impl CosignPin<'_> {
    #[must_use]
    pub fn url(&self) -> String {
        self.url_for(CosignPlatform::LinuxAmd64)
    }

    #[must_use]
    pub fn url_for(&self, platform: CosignPlatform) -> String {
        let asset = match platform {
            CosignPlatform::LinuxAmd64 => "cosign-linux-amd64",
            CosignPlatform::LinuxArm64 => "cosign-linux-arm64",
        };
        format!(
            "https://github.com/sigstore/cosign/releases/download/v{}/{asset}",
            self.version,
        )
    }

    fn is_valid(&self) -> bool {
        let numeric = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        self.version.split('.').count() == 3
            && self.version.split('.').all(numeric)
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && self.size > 0
    }
}

pub const PROVENANCE_DIR: &str = "$RUNNER_TEMP/provenance";
pub const BUNDLE_SUFFIX: &str = ".sigstore.json";
pub const RECEIPT_SUFFIX: &str = ".provenance.json";
pub const BUNDLE_MEDIA_TYPE: &str = "application/vnd.dev.sigstore.bundle.v0.3+json";
pub const PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";

const SIGNING_CONFIG: &str = r#"{"mediaType":"application/vnd.dev.sigstore.signingconfig.v0.2+json","caUrls":[{"url":"https://fulcio.githubapp.com","majorApiVersion":1,"validFor":{"start":"2023-01-01T00:00:00Z"},"operator":"github.com"}],"oidcUrls":[],"rekorTlogUrls":[],"tsaUrls":[{"url":"https://timestamp.githubapp.com/api/v1/timestamp","majorApiVersion":1,"validFor":{"start":"2023-01-01T00:00:00Z"},"operator":"github.com"}],"rekorTlogConfig":{"selector":"ANY"},"tsaConfig":{"selector":"ANY"}}"#;

const GITHUB_ROOT_FILTER: &str = r#"select((.certificateAuthorities // []) | length > 0 and all(.uri == "fulcio.githubapp.com"))"#;

const PREDICATE: &str = r#"{buildDefinition: {buildType: "https://actions.github.io/buildtypes/workflow/v1", externalParameters: {workflow: {ref: env.GITHUB_REF, repository: ("https://github.com/" + env.GITHUB_REPOSITORY), path: $path}}, internalParameters: {github: {event_name: env.GITHUB_EVENT_NAME, repository_id: env.GITHUB_REPOSITORY_ID, repository_owner_id: env.GITHUB_REPOSITORY_OWNER_ID, runner_environment: env.RUNNER_ENVIRONMENT}}, resolvedDependencies: [{uri: ("git+https://github.com/" + env.GITHUB_REPOSITORY + "@" + env.GITHUB_REF), digest: {gitCommit: env.GITHUB_SHA}}]}, runDetails: {builder: {id: ("https://github.com/actions/runner/" + (env.RUNNER_ENVIRONMENT // "unknown"))}, metadata: {invocationId: ("https://github.com/" + env.GITHUB_REPOSITORY + "/actions/runs/" + env.GITHUB_RUN_ID + "/attempts/" + env.GITHUB_RUN_ATTEMPT)}}}"#;

const RECEIPT: &str = r#"{schema: 1, kind: "velnor-publish-provenance", provenance: "sigstore_bundle", reason: "declared [publish] provenance = sigstore_bundle: the GitHub attestation store is not used", sigstore_instance: "github", predicate_type: $predicate, subject: {name: $name, sha256: $subject}, bundle: {name: ($name + $suffix), sha256: $bundle, media_type: $media}, source: {repository: env.GITHUB_REPOSITORY, repository_id: env.GITHUB_REPOSITORY_ID, repository_owner_id: env.GITHUB_REPOSITORY_OWNER_ID, ref: env.GITHUB_REF, sha: env.GITHUB_SHA}, workflow: {path: $path, ref: env.GITHUB_WORKFLOW_REF, sha: env.GITHUB_WORKFLOW_SHA}, run: {id: env.GITHUB_RUN_ID, attempt: env.GITHUB_RUN_ATTEMPT}, verify: ("gh attestation verify " + $name + " --bundle " + $name + $suffix + " --repo " + env.GITHUB_REPOSITORY + " --source-digest " + env.GITHUB_SHA + " --source-ref " + env.GITHUB_REF + " --signer-workflow " + env.GITHUB_REPOSITORY + "/" + $path + " --signer-digest " + env.GITHUB_WORKFLOW_SHA + " --predicate-type " + $predicate + " --format json")}"#;

const VERIFIED_IDENTITY_FILTER: &str = r#"def canonical_id: type == "string" and test("^[1-9][0-9]*$"); type == "array" and length > 0 and ($repository_id | canonical_id) and ($owner_id | canonical_id) and all(.[]; (.verificationResult.signature.certificate.sourceRepositoryIdentifier | canonical_id and . == $repository_id) and (.verificationResult.signature.certificate.sourceRepositoryOwnerIdentifier | canonical_id and . == $owner_id))"#;

fn verify_command(output: &str) -> String {
    format!(
        "gh attestation verify \"$s\" --bundle \"$p/$name{BUNDLE_SUFFIX}\" \
         --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" \
         --signer-workflow \"$GITHUB_REPOSITORY/{PUBLISH_WORKFLOW_PATH}\" \
         --signer-digest \"$GITHUB_WORKFLOW_SHA\" --predicate-type '{PREDICATE_TYPE}' \
         --format json > {output}"
    )
}

fn verified_identity_check(output: &str) -> String {
    format!(
        "jq -e --arg repository_id \"$GITHUB_REPOSITORY_ID\" \
         --arg owner_id \"$GITHUB_REPOSITORY_OWNER_ID\" \
         '{VERIFIED_IDENTITY_FILTER}' {output} > /dev/null"
    )
}

fn prelude(gh: &str, dir: &str, file: &str) -> String {
    format!(
        "set -eu; {gh}; ver=${{GITHUB_REF_NAME##*-v}}; s=\"{dir}/{file}\"; test -f \"$s\"; \
         test ! -L \"$s\"; name=\"${{s##*/}}\"; p=\"{PROVENANCE_DIR}\""
    )
}

pub fn sign_script(
    gh: &str,
    dir: &str,
    file: &str,
    cosign: CosignPin<'_>,
) -> Result<String, &'static str> {
    sign_script_for_platform(gh, dir, file, cosign, CosignPlatform::LinuxAmd64)
}

pub fn sign_script_for_platform(
    gh: &str,
    dir: &str,
    file: &str,
    cosign: CosignPin<'_>,
    platform: CosignPlatform,
) -> Result<String, &'static str> {
    if !cosign.is_valid() {
        return Err("invalid_cosign_pin");
    }
    let prelude = prelude(gh, dir, file);
    let verify = verify_command("\"$w/verification.json\"");
    let identity_check = verified_identity_check("\"$w/verification.json\"");
    let url = cosign.url_for(platform);
    let size = cosign.size;
    let sha = cosign.sha256;
    Ok(format!(
        "{prelude}; w=\"$RUNNER_TEMP/velnor-sign\"; rm -rf -- \"$p\" \"$w\"; mkdir -p -- \"$p\" \"$w\"; \
         curl --disable --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
         --tlsv1.2 --retry 5 --retry-all-errors --max-time 300 --max-filesize {size} \
         -o \"$w/cosign\" '{url}'; \
         wc -c < \"$w/cosign\" > \"$w/cosign.bytes\"; \
         read -r actual_size < \"$w/cosign.bytes\"; \
         case \"$actual_size\" in ''|*[!0-9]*) exit 1 ;; esac; \
         test \"$actual_size\" -eq {size}; \
         printf '%s  %s\\n' '{sha}' \"$w/cosign\" | sha256sum -c - > /dev/null; \
         chmod 700 \"$w/cosign\"; \
         gh attestation trusted-root > \"$w/roots.jsonl\"; \
         jq -c '{GITHUB_ROOT_FILTER}' \"$w/roots.jsonl\" > \"$w/root.json\"; \
         wc -l < \"$w/root.json\" | grep -Eqx ' *1'; \
         printf '%s\\n' '{SIGNING_CONFIG}' > \"$w/signing.json\"; \
         jq -n --arg path '{PUBLISH_WORKFLOW_PATH}' '{PREDICATE}' > \"$w/predicate.json\"; \
         \"$w/cosign\" attest-blob --yes --signing-config \"$w/signing.json\" \
         --trusted-root \"$w/root.json\" --type slsaprovenance1 --predicate \"$w/predicate.json\" \
         --bundle \"$p/$name{BUNDLE_SUFFIX}\" \"$s\"; \
         jq -e --arg media '{BUNDLE_MEDIA_TYPE}' '.mediaType == $media' \"$p/$name{BUNDLE_SUFFIX}\" > /dev/null; \
         {verify}; {identity_check}; \
         sha256sum < \"$s\" > \"$w/subject.sum\"; read -r subject rest < \"$w/subject.sum\"; \
         sha256sum < \"$p/$name{BUNDLE_SUFFIX}\" > \"$w/bundle.sum\"; read -r bundle rest < \"$w/bundle.sum\"; \
         jq -n --arg name \"$name\" --arg subject \"$subject\" --arg bundle \"$bundle\" \
         --arg suffix '{BUNDLE_SUFFIX}' --arg media '{BUNDLE_MEDIA_TYPE}' \
         --arg predicate '{PREDICATE_TYPE}' --arg path '{PUBLISH_WORKFLOW_PATH}' \
         '{RECEIPT}' > \"$p/$name{RECEIPT_SUFFIX}\"; rm -rf -- \"$w\""
    ))
}

#[must_use]
pub fn verify_script(gh: &str, dir: &str, file: &str) -> String {
    let prelude = prelude(gh, dir, file);
    let verify = verify_command("\"$w/verification.json\"");
    let identity_check = verified_identity_check("\"$w/verification.json\"");
    format!(
        "{prelude}; w=\"$RUNNER_TEMP/velnor-verify\"; mkdir -m 700 -- \"$w\"; \
         trap 'rm -rf -- \"$w\"' EXIT; \
         ls -- \"$p\" | wc -l | grep -Eqx ' *2'; {verify}; {identity_check}; {receipt}",
        receipt = receipt_check()
    )
}

fn receipt_check() -> String {
    format!(
        "sha256sum < \"$s\" > \"$p.subject.sum\"; read -r subject rest < \"$p.subject.sum\"; \
         sha256sum < \"$p/$name{BUNDLE_SUFFIX}\" > \"$p.bundle.sum\"; read -r bundle rest < \"$p.bundle.sum\"; \
         rm -f -- \"$p.subject.sum\" \"$p.bundle.sum\"; \
         jq -e --arg name \"$name\" --arg subject \"$subject\" --arg bundle \"$bundle\" \
         --arg repository_id \"$GITHUB_REPOSITORY_ID\" \
         --arg owner_id \"$GITHUB_REPOSITORY_OWNER_ID\" \
         '{RECEIPT_CHECK_FILTER}' \
         \"$p/$name{RECEIPT_SUFFIX}\" > /dev/null"
    )
}

const RECEIPT_CHECK_FILTER: &str = r#"def canonical_id: type == "string" and test("^[1-9][0-9]*$"); .provenance == "sigstore_bundle" and .subject.name == $name and .subject.sha256 == $subject and .bundle.sha256 == $bundle and .source.repository == env.GITHUB_REPOSITORY and .source.sha == env.GITHUB_SHA and .source.ref == env.GITHUB_REF and ($repository_id | canonical_id) and ($owner_id | canonical_id) and .source.repository_id == $repository_id and .source.repository_owner_id == $owner_id"#;

#[must_use]
pub fn receipt_script(dir: &str, file: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; s=\"{dir}/{file}\"; test -f \"$s\"; \
         name=\"${{s##*/}}\"; p=\"{PROVENANCE_DIR}\"; ls -- \"$p\" | wc -l | grep -Eqx ' *2'; {}",
        receipt_check()
    )
}

#[must_use]
pub fn stage_script(dir: &str, file: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; s=\"{dir}/{file}\"; name=\"${{s##*/}}\"; \
         p=\"{PROVENANCE_DIR}\"; cp -- \"$p/$name{BUNDLE_SUFFIX}\" \"$p/$name{RECEIPT_SUFFIX}\" \"{dir}/\""
    )
}
