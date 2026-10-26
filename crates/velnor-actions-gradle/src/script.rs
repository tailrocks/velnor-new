// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const INIT_SCRIPT_NAME: &str = "velnor.init.gradle.kts";

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

#[must_use]
pub fn user_home_properties() -> &'static str {
    "org.gradle.java.installations.auto-download=false\n\
     org.gradle.java.installations.auto-detect=false\n\
     org.gradle.java.installations.fromEnv=JAVA_HOME\n"
}
