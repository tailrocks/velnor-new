//! The wrapper-verification step, executed with `sh` against fixture trees.

use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_gradle::script::{init_script, user_home_properties, verify_wrapper_script};

const JAR: &[u8] = include_bytes!("fixtures/gradle-wrapper-9.5.1.jar");
const BIN: &str = "bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f";
const ALL: &str = "c72fb9991f6025cbe337d52ba77e531b3faf62bdd3e348fe1ccee9f51c71adb0";

fn write_tree(dir: &Path, props: &str, jar: &[u8]) {
    let wrapper = dir.join("gradle/wrapper");
    fs::create_dir_all(&wrapper).expect("dirs");
    fs::write(wrapper.join("gradle-wrapper.properties"), props).expect("props");
    fs::write(wrapper.join("gradle-wrapper.jar"), jar).expect("jar");
}

fn props(flavour: &str, sum: &str) -> String {
    format!(
        "distributionBase=GRADLE_USER_HOME\ndistributionSha256Sum={sum}\n\
         distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-{flavour}.zip\n"
    )
}

/// Run the step with the tree as its root; returns (success, stderr).
fn run(dir: &Path) -> (bool, String) {
    let script = verify_wrapper_script(dir.to_str().expect("utf8 path"));
    let out = Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("sh");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn scratch() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("velnor-gradle-script-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

#[test]
fn accepts_the_official_wrapper_for_both_flavours() {
    for (flavour, sum) in [("bin", BIN), ("all", ALL)] {
        let dir = scratch();
        write_tree(&dir, &props(flavour, sum), JAR);
        assert_eq!(run(&dir), (true, String::new()), "{flavour}");
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}

#[test]
fn every_gate_fails_closed_with_its_code() {
    let mut tampered = JAR.to_vec();
    tampered[200] ^= 1;
    let url_bad = props("bin", BIN).replace("services.gradle.org", "evil.example");
    let no_sum =
        "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-bin.zip\n";
    let unknown = props("bin", BIN).replace("9.5.1", "1.1");
    let cases = [
        (props("bin", BIN), tampered, "gradle_wrapper_jar_unverified"),
        (
            props("bin", &"0".repeat(64)),
            JAR.to_vec(),
            "gradle_distribution_checksum_mismatch",
        ),
        (
            props("bin", ALL),
            JAR.to_vec(),
            "gradle_distribution_checksum_mismatch",
        ),
        (url_bad, JAR.to_vec(), "gradle_distribution_url_untrusted"),
        (
            no_sum.to_owned(),
            JAR.to_vec(),
            "gradle_distribution_checksum_missing",
        ),
        (unknown, JAR.to_vec(), "gradle_distribution_uncataloged"),
    ];
    for (properties, jar, code) in cases {
        let dir = scratch();
        write_tree(&dir, &properties, &jar);
        let (ok, stderr) = run(&dir);
        assert!(!ok && stderr.contains(code), "{code}: {stderr}");
        fs::remove_dir_all(&dir).expect("cleanup");
    }
    let empty = scratch();
    let (ok, stderr) = run(&empty);
    assert!(
        !ok && stderr.contains("gradle_wrapper_properties_missing"),
        "{stderr}"
    );
    fs::remove_dir_all(&empty).expect("cleanup");
}

#[test]
fn init_script_and_properties_are_fixed_text() {
    assert!(init_script().contains("VELNOR_MAVEN_ROOT"));
    assert!(init_script().contains("com\\\\.chainargos"));
    assert!(user_home_properties().contains("auto-download=false"));
}
