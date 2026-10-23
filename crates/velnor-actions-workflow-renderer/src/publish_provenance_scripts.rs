//! Fixed shell text for `[publish] provenance = "sigstore_bundle"`.
//!
//! The declared fallback for repositories whose plan has no GitHub attestation
//! store. The signature is unchanged in kind: keyless, bound to the job's
//! GitHub OIDC identity, issued by GitHub's Sigstore instance (Fulcio
//! `fulcio.githubapp.com`, timestamp authority `timestamp.githubapp.com`, no
//! transparency log, so a private repository's identity never reaches a
//! public log). Only the store differs: instead of `POST /attestations`, the
//! signed bundle and a provenance receipt travel as a run artifact and are
//! recorded as immutable assets of the tag's GitHub release.
//!
//! Every script is a constant template. Its only inputs are validated
//! artifact names, fixed directories and the pinned Cosign digest, so no
//! config text can introduce shell syntax. The verifier is
//! `gh attestation verify --bundle`, which selects GitHub's trusted root
//! from the certificate issuer and checks the certificate's source
//! repository, source digest, source ref, signer workflow and signer digest.

use crate::publish_render::PUBLISH_WORKFLOW_PATH;

/// The pinned Cosign Linux `x86_64` release asset the sign job admits.
///
/// The caller passes the catalog pin (`velnor_actions_mise`); the script
/// downloads exactly `size` bytes at most and requires the exact `sha256`.
#[derive(Debug, Clone, Copy)]
pub struct CosignPin<'a> {
    /// Release version without the `v` (`3.1.4`).
    pub version: &'a str,
    /// Lowercase hex sha256 of the asset.
    pub sha256: &'a str,
    /// Exact asset size in bytes.
    pub size: usize,
}

impl CosignPin<'_> {
    /// Release asset URL of the pin.
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "https://github.com/sigstore/cosign/releases/download/v{}/cosign-linux-amd64",
            self.version
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

/// Directory (under runner temp) the bundle and the receipt travel in.
pub const PROVENANCE_DIR: &str = "$RUNNER_TEMP/provenance";
/// Suffix of the Sigstore bundle written next to a subject name.
pub const BUNDLE_SUFFIX: &str = ".sigstore.json";
/// Suffix of the provenance receipt written next to a subject name.
pub const RECEIPT_SUFFIX: &str = ".provenance.json";
/// Sigstore bundle media type the signer must produce.
pub const BUNDLE_MEDIA_TYPE: &str = "application/vnd.dev.sigstore.bundle.v0.3+json";
/// SLSA provenance predicate type signed into the in-toto statement.
pub const PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";

/// Signing config naming GitHub's Sigstore instance and no transparency log.
const SIGNING_CONFIG: &str = r#"{"mediaType":"application/vnd.dev.sigstore.signingconfig.v0.2+json","caUrls":[{"url":"https://fulcio.githubapp.com","majorApiVersion":1,"validFor":{"start":"2023-01-01T00:00:00Z"},"operator":"github.com"}],"oidcUrls":[],"rekorTlogUrls":[],"tsaUrls":[{"url":"https://timestamp.githubapp.com/api/v1/timestamp","majorApiVersion":1,"validFor":{"start":"2023-01-01T00:00:00Z"},"operator":"github.com"}],"rekorTlogConfig":{"selector":"ANY"},"tsaConfig":{"selector":"ANY"}}"#;

/// jq filter picking GitHub's trusted root out of `gh attestation trusted-root`.
const GITHUB_ROOT_FILTER: &str = r#"select((.certificateAuthorities // []) | length > 0 and all(.uri == "fulcio.githubapp.com"))"#;

/// SLSA v1 predicate built from the runner's own GitHub context.
const PREDICATE: &str = r#"{buildDefinition: {buildType: "https://actions.github.io/buildtypes/workflow/v1", externalParameters: {workflow: {ref: env.GITHUB_REF, repository: ("https://github.com/" + env.GITHUB_REPOSITORY), path: $path}}, internalParameters: {github: {event_name: env.GITHUB_EVENT_NAME, repository_id: env.GITHUB_REPOSITORY_ID, repository_owner_id: env.GITHUB_REPOSITORY_OWNER_ID, runner_environment: env.RUNNER_ENVIRONMENT}}, resolvedDependencies: [{uri: ("git+https://github.com/" + env.GITHUB_REPOSITORY + "@" + env.GITHUB_REF), digest: {gitCommit: env.GITHUB_SHA}}]}, runDetails: {builder: {id: ("https://github.com/actions/runner/" + (env.RUNNER_ENVIRONMENT // "unknown"))}, metadata: {invocationId: ("https://github.com/" + env.GITHUB_REPOSITORY + "/actions/runs/" + env.GITHUB_RUN_ID + "/attempts/" + env.GITHUB_RUN_ATTEMPT)}}}"#;

/// Receipt recording the declared mode, the exact bytes and how to verify them.
const RECEIPT: &str = r#"{schema: 1, kind: "velnor-publish-provenance", provenance: "sigstore_bundle", reason: "declared [publish] provenance = sigstore_bundle: the GitHub attestation store is not used", sigstore_instance: "github", predicate_type: $predicate, subject: {name: $name, sha256: $subject}, bundle: {name: ($name + $suffix), sha256: $bundle, media_type: $media}, source: {repository: env.GITHUB_REPOSITORY, ref: env.GITHUB_REF, sha: env.GITHUB_SHA}, workflow: {path: $path, ref: env.GITHUB_WORKFLOW_REF, sha: env.GITHUB_WORKFLOW_SHA}, run: {id: env.GITHUB_RUN_ID, attempt: env.GITHUB_RUN_ATTEMPT}, verify: ("gh attestation verify " + $name + " --bundle " + $name + $suffix + " --repo " + env.GITHUB_REPOSITORY + " --source-digest " + env.GITHUB_SHA + " --source-ref " + env.GITHUB_REF + " --signer-workflow " + env.GITHUB_REPOSITORY + "/" + $path + " --signer-digest " + env.GITHUB_WORKFLOW_SHA + " --predicate-type " + $predicate)}"#;

/// The exact `gh attestation verify` that admits `$s` with bundle `$p/$name.sigstore.json`.
fn verify_command() -> String {
    format!(
        "gh attestation verify \"$s\" --bundle \"$p/$name{BUNDLE_SUFFIX}\" \
         --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" \
         --signer-workflow \"$GITHUB_REPOSITORY/{PUBLISH_WORKFLOW_PATH}\" \
         --signer-digest \"$GITHUB_WORKFLOW_SHA\" --predicate-type '{PREDICATE_TYPE}' > /dev/null"
    )
}

/// Shell prelude: `$s` is the subject, `$name` its file name, `$p` the provenance dir.
///
/// `dir` is a fixed runner-temp directory and `file` a validated name that
/// may use `$ver` (the tag's version).
fn prelude(gh: &str, dir: &str, file: &str) -> String {
    format!(
        "set -eu; {gh}; ver=${{GITHUB_REF_NAME##*-v}}; s=\"{dir}/{file}\"; test -f \"$s\"; \
         test ! -L \"$s\"; name=\"${{s##*/}}\"; p=\"{PROVENANCE_DIR}\""
    )
}

/// Sign `dir/file` into a Sigstore bundle, verify it, and write the receipt.
///
/// The pinned Cosign is fetched over HTTPS and admitted by its exact
/// sha256. Signing uses the job's ambient GitHub OIDC token. The bundle is
/// verified at once with the same `gh attestation verify` the publish job
/// repeats; a bundle that does not verify fails the job before any receipt
/// exists.
///
/// # Errors
///
/// Returns `invalid_cosign_pin` when the pin is not a strict version,
/// lowercase sha256 and positive size.
pub fn sign_script(
    gh: &str,
    dir: &str,
    file: &str,
    cosign: CosignPin<'_>,
) -> Result<String, &'static str> {
    if !cosign.is_valid() {
        return Err("invalid_cosign_pin");
    }
    let prelude = prelude(gh, dir, file);
    let verify = verify_command();
    let url = cosign.url();
    let size = cosign.size;
    let sha = cosign.sha256;
    Ok(format!(
        "{prelude}; w=\"$RUNNER_TEMP/velnor-sign\"; rm -rf -- \"$p\" \"$w\"; mkdir -p -- \"$p\" \"$w\"; \
         curl --disable --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
         --tlsv1.2 --retry 5 --retry-all-errors --max-time 300 --max-filesize {size} \
         -o \"$w/cosign\" '{url}'; \
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
         {verify}; \
         sha256sum < \"$s\" > \"$w/subject.sum\"; read -r subject rest < \"$w/subject.sum\"; \
         sha256sum < \"$p/$name{BUNDLE_SUFFIX}\" > \"$w/bundle.sum\"; read -r bundle rest < \"$w/bundle.sum\"; \
         jq -n --arg name \"$name\" --arg subject \"$subject\" --arg bundle \"$bundle\" \
         --arg suffix '{BUNDLE_SUFFIX}' --arg media '{BUNDLE_MEDIA_TYPE}' \
         --arg predicate '{PREDICATE_TYPE}' --arg path '{PUBLISH_WORKFLOW_PATH}' \
         '{RECEIPT}' > \"$p/$name{RECEIPT_SUFFIX}\"; rm -rf -- \"$w\""
    ))
}

/// Re-verify the downloaded bundle and receipt against `dir/file` before publishing.
///
/// The provenance directory holds exactly the bundle and the receipt; the
/// bundle must pass the same `gh attestation verify`, and the receipt must
/// name this subject's and bundle's sha256 and this run's source commit.
#[must_use]
pub fn verify_script(gh: &str, dir: &str, file: &str) -> String {
    let prelude = prelude(gh, dir, file);
    let verify = verify_command();
    format!(
        "{prelude}; ls -- \"$p\" | wc -l | grep -Eqx ' *2'; {verify}; {receipt}",
        receipt = receipt_check()
    )
}

/// Receipt check without `gh`: subject and bundle sha256, mode and source commit.
fn receipt_check() -> String {
    format!(
        "sha256sum < \"$s\" > \"$p.subject.sum\"; read -r subject rest < \"$p.subject.sum\"; \
         sha256sum < \"$p/$name{BUNDLE_SUFFIX}\" > \"$p.bundle.sum\"; read -r bundle rest < \"$p.bundle.sum\"; \
         rm -f -- \"$p.subject.sum\" \"$p.bundle.sum\"; \
         jq -e --arg name \"$name\" --arg subject \"$subject\" --arg bundle \"$bundle\" \
         '.provenance == \"sigstore_bundle\" and .subject.name == $name and .subject.sha256 == $subject \
         and .bundle.sha256 == $bundle and .source.sha == env.GITHUB_SHA and .source.ref == env.GITHUB_REF' \
         \"$p/$name{RECEIPT_SUFFIX}\" > /dev/null"
    )
}

/// Receipt check for a job without `gh` (the npm push): binds the pushed
/// bytes to the receipt the provenance job already verified and recorded.
#[must_use]
pub fn receipt_script(dir: &str, file: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; s=\"{dir}/{file}\"; test -f \"$s\"; \
         name=\"${{s##*/}}\"; p=\"{PROVENANCE_DIR}\"; ls -- \"$p\" | wc -l | grep -Eqx ' *2'; {}",
        receipt_check()
    )
}

/// Copy the verified bundle and receipt beside the subject for the release upload.
#[must_use]
pub fn stage_script(dir: &str, file: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; s=\"{dir}/{file}\"; name=\"${{s##*/}}\"; \
         p=\"{PROVENANCE_DIR}\"; cp -- \"$p/$name{BUNDLE_SUFFIX}\" \"$p/$name{RECEIPT_SUFFIX}\" \"{dir}/\""
    )
}
