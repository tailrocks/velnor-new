// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{upload_assets_script, verify_sidecar_script};

#[must_use]
pub fn zip_name(id: &str) -> String {
    format!("{id}-$ver-maven.zip")
}

#[must_use]
pub fn collect_script(id: &str, output_dir: &str) -> String {
    let name = zip_name(id);
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; out=\"$RUNNER_TEMP/bundle\"; mkdir -p \"$out\"; \
         poms=\"$RUNNER_TEMP/maven-poms.txt\"; cd -P -- \"$GITHUB_WORKSPACE/{output_dir}\"; \
         name=\"{name}\"; test -f \"$name\"; test -f \"$name.sha256\"; \
         sha256sum -c -- \"$name.sha256\"; \
         unzip -Z1 -- \"$name\" | {{ if grep -v '^repository/'; then \
         printf 'velnor: entry outside repository/\\n' >&2; exit 1; fi; }}; \
         unzip -Z1 -- \"$name\" | grep -E '\\.pom$' > \"$poms\" \
         || {{ printf 'velnor: bundle holds no pom\\n' >&2; exit 1; }}; \
         grep -Fv -e \"/$ver/\" \"$poms\" | {{ if grep .; then \
         printf 'velnor: pom outside version %s\\n' \"$ver\" >&2; exit 1; fi; }}; \
         rm -f -- \"$poms\"; mv -- \"$name\" \"$name.sha256\" \"$out/\""
    )
}

#[must_use]
pub fn verify_script(id: &str) -> String {
    verify_sidecar_script(&zip_name(id))
}

#[must_use]
pub fn upload_script(gh: &str, id: &str) -> String {
    upload_assets_script(gh, &zip_name(id))
}

#[must_use]
pub fn attest_disabled_script(reason: &str) -> String {
    format!(
        "set -eu; printf '::warning title=Provenance attestation disabled::%s\\n' 'publish.maven attest=false: {reason}'"
    )
}
