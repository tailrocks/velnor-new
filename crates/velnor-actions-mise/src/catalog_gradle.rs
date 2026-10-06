//! Exact consumer Gradle engine and wrapper bootstrap authority.
//!
//! The consumer's bootstrap is generated from Gradle 9.4.1 and downloads
//! Gradle 9.5.1. These are distinct version authorities. The script is the
//! inspected project-generated template with its exact JVM options; it is
//! not byte-identical to Gradle's checked-in root `gradlew`.
//! Verified official tag, template, JAR and distribution checksum 2026-10-03.

/// Engine version selected by the consumer's wrapper properties.
pub const GRADLE_WRAPPER_VERSION: &str = "9.5.1";
/// Binary distribution checksum published by Gradle for engine 9.5.1.
pub const GRADLE_WRAPPER_DISTRIBUTION_SHA256: &str =
    "bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f";
/// Exact qualified project-generated shell bootstrap bytes.
pub const GRADLE_WRAPPER_SCRIPT_SHA256: &str =
    "aed171fb114f82e6eaea4970a245a200e0582a7dcc8ec0891ca41b6e4a62b754";
/// Exact official Gradle 9.4.1 wrapper JAR bytes used by the consumer.
pub const GRADLE_WRAPPER_JAR_SHA256: &str =
    "55243ef57851f12b070ad14f7f5bb8302daceeebc5bce5ece5fa6edb23e1145c";
/// Upstream release which generated the consumer bootstrap.
pub const BOOTSTRAP_VERSION: &str = "9.4.1";
/// Official Gradle 9.4.1 source commit recorded in the generated script.
pub const BOOTSTRAP_SOURCE_SHA: &str = "2d6327017519d23b96af35865dc997fcb544fb40";
/// Immutable upstream template used by the consumer-generated script.
pub const BOOTSTRAP_TEMPLATE_SOURCE: &str = "https://github.com/gradle/gradle/blob/2d6327017519d23b96af35865dc997fcb544fb40/platforms/jvm/plugins-application/src/main/resources/org/gradle/api/internal/plugins/unixStartScript.txt";
/// Immutable official wrapper JAR source.
pub const BOOTSTRAP_JAR_SOURCE: &str = "https://raw.githubusercontent.com/gradle/gradle/2d6327017519d23b96af35865dc997fcb544fb40/gradle/wrapper/gradle-wrapper.jar";
/// Official engine distribution checksum authority.
pub const DISTRIBUTION_CHECKSUM_SOURCE: &str =
    "https://services.gradle.org/distributions/gradle-9.5.1-bin.zip.sha256";

/// Exact PostgreSQL image approved for the source-bound Gradle fixture.
pub const POSTGRES_FIXTURE_IMAGE: &str =
    "postgres:18.6-trixie@sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280";
