use super::*;
use crate::setup::MiseSetup;
use crate::yaml::render_yaml;

fn pins() -> MisePinQualificationPins {
    MisePinQualificationPins {
        linux_x86_64_setup: MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: "2026.10.5".to_owned(),
            sha256: "b".repeat(64),
        },
        macos_x86_64_setup: MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: "2026.10.5".to_owned(),
            sha256: "c".repeat(64),
        },
    }
}

#[test]
fn jobs_are_opt_in_exact_ref_read_only_and_no_publication() {
    let jobs = jobs(&pins()).expect("valid pins render");
    assert_eq!(
        jobs.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["mise-pin-linux-x64", "mise-pin-macos-x64"]
    );

    for (id, body) in &jobs {
        let rendered = render_yaml(body);
        assert!(rendered.contains("if: inputs.mode == 'mise-pin'"), "{id}");
        assert!(rendered.contains("permissions:\n  contents: read"), "{id}");
        assert!(rendered.contains("ref: ${{ github.sha }}"), "{id}");
        assert!(rendered.contains("persist-credentials: \"false\""), "{id}");
        assert!(rendered.contains("version: 2026.10.5"), "{id}");
        assert!(rendered.contains("install: \"false\""), "{id}");
        assert!(rendered.contains("env: \"false\""), "{id}");
        assert!(rendered.contains("cache: \"false\""), "{id}");
        assert!(rendered.contains("cache_save: \"false\""), "{id}");
        assert!(rendered.contains("git rev-parse HEAD"), "{id}");
        assert!(rendered.contains("mise --version"), "{id}");
        assert!(rendered.contains("MISE_SHA256"), "{id}");
        assert!(!rendered.contains("secrets."), "{id}");
        assert!(!rendered.contains("actions: write"), "{id}");
        assert!(!rendered.contains("artifact"), "{id}");
        assert!(!rendered.contains("publish"), "{id}");
    }

    let linux = render_yaml(&jobs[0].1);
    let macos = render_yaml(&jobs[1].1);
    assert!(linux.contains("runs-on: ubuntu-26.04"));
    assert!(linux.contains(&format!("sha256: {}", "b".repeat(64))));
    assert!(linux.contains("sha256sum"));
    assert!(macos.contains("runs-on: macos-15-intel"));
    assert!(macos.contains(&format!("sha256: {}", "c".repeat(64))));
    assert!(macos.contains("shasum -a 256"));
}

#[test]
fn mismatched_candidate_versions_fail_closed() {
    let mut pins = pins();
    pins.macos_x86_64_setup.version = "2026.10.6".to_owned();
    assert!(jobs(&pins).is_err_and(|error| {
        error
            .to_string()
            .contains("mise_pin_qualification_version_mismatch")
    }));
}
