//! Fixed Gradle init text and user-home properties for generated Gradle jobs.
//!
//! Everything here is a pure function of the catalog and the Gradle root
//! (a validated relative path), never of repository content, so the text a
//! job runs is reviewable in the generated workflow.

/// Name of the Velnor-owned init script written under `$GRADLE_USER_HOME/init.d`.
pub const INIT_SCRIPT_NAME: &str = "velnor.init.gradle.kts";

/// The Velnor-owned init script: adds every fetched Maven bundle repository
/// (`$VELNOR_MAVEN_ROOT/<id>/repository`) for plugins and dependencies,
/// restricted to the `com.chainargos` group, so consumer settings need no
/// path knowledge of where the job unpacked them.
#[must_use]
pub fn init_script() -> &'static str {
    r#"val velnorMavenRoot = System.getenv("VELNOR_MAVEN_ROOT")
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
"#
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
