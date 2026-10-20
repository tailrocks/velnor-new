//! Fixed shell and Gradle init text for generated Gradle jobs.
//!
//! Everything here is a pure function of the catalog and the Gradle root
//! (a validated relative path), never of repository content, so the text a
//! job runs is reviewable in the generated workflow.

use crate::catalog::RELEASES;

/// Name of the Velnor-owned init script written under `$GRADLE_USER_HOME/init.d`.
pub const INIT_SCRIPT_NAME: &str = "velnor.init.gradle.kts";

/// `    <version>) want=<digest> ;;` rows of one shell `case`.
fn rows(select: impl Fn(&crate::catalog::GradleRelease) -> &'static str) -> String {
    RELEASES
        .iter()
        .map(|row| format!("    {}) want={} ;;\n", row.version, select(row)))
        .collect()
}

/// Wrapper gates 1-3 as a job step: official URL, catalog distribution digest,
/// official wrapper-jar digest. Runs before `./gradlew` ever executes.
///
/// `root` must be a validated repository-relative directory limited to
/// `[A-Za-z0-9._/-]`, so no quoting is needed.
#[must_use]
pub fn verify_wrapper_script(root: &str) -> String {
    let bins = rows(|row| row.bin_sha256);
    let alls = rows(|row| row.all_sha256);
    let mut jars: Vec<&str> = RELEASES.iter().map(|row| row.wrapper_jar_sha256).collect();
    jars.sort_unstable();
    jars.dedup();
    let jar_cases = jars.join("|");
    let root = if root.is_empty() { "." } else { root };
    format!(
        r##"set -eu
fail() {{ printf 'velnor: %s\n' "$1" >&2; exit 1; }}
root={root}
props="$root/gradle/wrapper/gradle-wrapper.properties"
jar="$root/gradle/wrapper/gradle-wrapper.jar"
[ -f "$props" ] && [ ! -L "$props" ] || fail gradle_wrapper_properties_missing
[ -f "$jar" ] && [ ! -L "$jar" ] || fail gradle_wrapper_jar_unverified
url=$(sed -n 's/^distributionUrl=//p' "$props" | sed 's/\\:/:/')
sum=$(sed -n 's/^distributionSha256Sum=//p' "$props")
base=https://services.gradle.org/distributions/gradle-
case "$url" in
  "$base"*-bin.zip) flavour=bin; ver=${{url#"$base"}}; ver=${{ver%-bin.zip}} ;;
  "$base"*-all.zip) flavour=all; ver=${{url#"$base"}}; ver=${{ver%-all.zip}} ;;
  *) fail gradle_distribution_url_untrusted ;;
esac
[ -n "$sum" ] || fail gradle_distribution_checksum_missing
want=
if [ "$flavour" = bin ]; then
  case "$ver" in
{bins}    *) fail gradle_distribution_uncataloged ;;
  esac
else
  case "$ver" in
{alls}    *) fail gradle_distribution_uncataloged ;;
  esac
fi
[ "$sum" = "$want" ] || fail gradle_distribution_checksum_mismatch
if command -v sha256sum >/dev/null 2>&1; then jar_sum=$(sha256sum < "$jar"); else jar_sum=$(shasum -a 256 < "$jar"); fi
jar_sum=${{jar_sum%% *}}
case "$jar_sum" in
  {jar_cases}) ;;
  *) fail gradle_wrapper_jar_unverified ;;
esac
printf 'velnor: gradle %s (%s) wrapper verified\n' "$ver" "$flavour"
"##
    )
}

/// The Velnor-owned init script: adds every fetched Maven bundle repository
/// (`$VELNOR_MAVEN_ROOT/<id>/repository`) for plugins and dependencies,
/// restricted to the `com.chainargos` group, so consumer settings need no
/// path knowledge of where the job unpacked them.
#[must_use]
pub fn init_script() -> &'static str {
    r##"val velnorMavenRoot = System.getenv("VELNOR_MAVEN_ROOT")
fun bundleRepositories(): List<java.io.File> =
    if (velnorMavenRoot == null) emptyList()
    else java.io.File(velnorMavenRoot).listFiles()
        .orEmpty()
        .map { java.io.File(it, "repository") }
        .filter { it.isDirectory }
        .sortedBy { it.path }
beforeSettings {
    bundleRepositories().forEach { dir ->
        pluginManagement.repositories {
            maven {
                url = uri(dir)
                content { includeGroupByRegex("com\\.chainargos(\\..+)?") }
            }
        }
        dependencyResolutionManagement.repositories {
            maven {
                url = uri(dir)
                content { includeGroupByRegex("com\\.chainargos(\\..+)?") }
            }
        }
    }
}
"##
}

/// Velnor-owned `gradle.properties` for `$GRADLE_USER_HOME`: forces Gradle
/// toolchains onto the installed JDK. User-home properties outrank project
/// properties, so the repository cannot undo this.
#[must_use]
pub fn user_home_properties() -> &'static str {
    "org.gradle.java.installations.auto-download=false\n\
     org.gradle.java.installations.auto-detect=false\n\
     org.gradle.java.installations.fromEnv=JAVA_HOME\n"
}
