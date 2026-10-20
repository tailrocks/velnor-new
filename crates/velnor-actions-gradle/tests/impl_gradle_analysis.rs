//! Whole-root analysis over in-memory fixture trees with the real 9.5.1 wrapper jar.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_gradle::analysis::*;

const JAR: &[u8] = include_bytes!("fixtures/gradle-wrapper-9.5.1.jar");
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn tree() -> BTreeMap<String, Vec<u8>> {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut put = |path: &str, text: &str| {
        files.insert(path.to_owned(), text.as_bytes().to_vec());
    };
    put(
        "gradle/wrapper/gradle-wrapper.properties",
        "distributionSha256Sum=bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f\n\
         distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-bin.zip\n",
    );
    put(
        "settings.gradle.kts",
        "rootProject.name = \"m\"\ninclude(\"core\", \"app\")\n",
    );
    put("core/build.gradle.kts", "dependencies { api(libs.jooq) }\n");
    put("core/gradle.lockfile", "# lock\n");
    put(
        "app/build.gradle.kts",
        "dependencies { api(projects.core) }\n",
    );
    put("app/gradle.lockfile", "# lock\n");
    put(
        "gradle/verification-metadata.xml",
        &format!(
            "<verification-metadata><configuration><verify-metadata>true</verify-metadata></configuration>\
             <components><component group=\"g\" name=\"n\" version=\"1\"><artifact name=\"n-1.jar\">\
             <sha256 value=\"{SHA}\"/></artifact></component></components></verification-metadata>"
        ),
    );
    files.insert("gradle/wrapper/gradle-wrapper.jar".to_owned(), JAR.to_vec());
    files
}

type Mutation = fn(&mut BTreeMap<String, Vec<u8>>);

fn run(files: &BTreeMap<String, Vec<u8>>) -> RootAnalysis {
    let names: BTreeSet<String> = files.keys().cloned().collect();
    analyze_root("backend", &names, &|path| files.get(path).cloned())
}

#[test]
fn a_compliant_root_has_no_findings_and_a_dependency_graph() {
    let analysis = run(&tree());
    assert_eq!(analysis.findings, Vec::<String>::new());
    assert_eq!(
        analysis.wrapper.as_ref().map(|w| w.version.as_str()),
        Some("9.5.1")
    );
    let app: Vec<_> = analysis.edges[":app"].iter().map(String::as_str).collect();
    assert_eq!(app, [":core"]);
    assert!(analysis.edges[":core"].is_empty());
}

#[test]
fn each_gate_reports_its_own_code() {
    let cases: [(&str, Mutation, &str); 6] = [
        (
            "tampered wrapper",
            |f| {
                f.get_mut("gradle/wrapper/gradle-wrapper.jar").expect("jar")[100] ^= 1;
            },
            "gradle_wrapper_jar_unverified",
        ),
        (
            "dynamic settings",
            |f| {
                f.insert(
                    "settings.gradle.kts".into(),
                    b"rootDir.listFiles()!!.forEach { include(it.name) }".to_vec(),
                );
            },
            "gradle_dynamic_settings:construct:listFiles",
        ),
        (
            "missing lock",
            |f| {
                f.remove("app/gradle.lockfile");
            },
            "gradle_dependency_lock_missing::app",
        ),
        (
            "missing metadata",
            |f| {
                f.remove("gradle/verification-metadata.xml");
            },
            "gradle_verification_metadata_missing",
        ),
        (
            "no checksum pin",
            |f| {
                f.insert(
                "gradle/wrapper/gradle-wrapper.properties".into(),
                b"distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-bin.zip\n".to_vec(),
            );
            },
            "gradle_distribution_checksum_missing",
        ),
        (
            "no settings",
            |f| {
                f.remove("settings.gradle.kts");
            },
            "gradle_settings_missing",
        ),
    ];
    for (name, mutate, code) in cases {
        let mut files = tree();
        mutate(&mut files);
        assert!(
            run(&files).findings.iter().any(|f| f == code),
            "{name}: {code}"
        );
    }
}
