//! Fixed shell text for the Maven bundle publication jobs (hard decision 8).
//!
//! The root's own `:mavenBundle` task writes `<id>-<version>-maven.zip` and a
//! `.sha256` under its `build/maven-bundle/`. The build job re-verifies both,
//! checks that the zip is one `repository/` tree whose POMs all sit at the
//! tag's version, and moves exactly those two files into the shared bundle
//! directory. Attest and publish work from that directory alone and execute no
//! repository code. Templates are filled only with values the contract
//! validated (artifact id, repo-relative output directory).

use super::{upload_assets_script, verify_sidecar_script};

/// Zip file name for artifact `id`; `$ver` is the tag's version shell variable.
#[must_use]
pub fn zip_name(id: &str) -> String {
    format!("{id}-$ver-maven.zip")
}

/// Verify the built bundle and move it, with its checksum, into the bundle directory.
///
/// `output_dir` is the repository-relative `build/maven-bundle` of the Gradle root.
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

/// Re-verify the checksum sidecar against the downloaded zip.
#[must_use]
pub fn verify_script(id: &str) -> String {
    verify_sidecar_script(&zip_name(id))
}

/// Upload the zip and checksum as immutable assets of the tag's GitHub release.
#[must_use]
pub fn upload_script(gh: &str, id: &str) -> String {
    upload_assets_script(gh, &zip_name(id))
}
