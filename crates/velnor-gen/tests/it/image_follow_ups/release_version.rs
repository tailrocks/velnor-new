// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_gen::expected_files;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, image, jobs, name_of, sample, steps};

fn at_helper(name: &str, helper: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut view = sample(name)?;
    let lock = String::from_utf8(view.files.remove(&LOCK_PATH).ok_or("no lock")?)?;
    view.files.insert(
        LOCK_PATH,
        lock.replace("\"0.1.116\"", &format!("\"{helper}\""))
            .into_bytes(),
    );
    assert!(expected_files(&view).is_ok());
    image(&view)
}

fn runs(job: &Value) -> Vec<&str> {
    steps(job)
        .into_iter()
        .filter_map(|step| step["run"].as_str())
        .collect()
}

#[test]
fn from_helper_0_1_117_a_release_tag_is_checked_against_the_crate_version_before_promote()
-> TestResult {
    for name in SAMPLES {
        let workflow = at_helper(name, "0.1.117")?;
        let image = name_of(name);
        let version = &workflow["jobs"][format!("image-{image}-version")];
        assert!(version.is_object(), "{name}");
        assert_eq!(
            version["needs"],
            json!([format!("image-{image}-authority")]),
            "{name}"
        );
        assert!(version.get("environment").is_none(), "{name}");
        assert_eq!(
            version["permissions"],
            json!({"contents": "read"}),
            "{name}"
        );
        let checks: Vec<&str> = runs(version)
            .into_iter()
            .filter(|run| run.contains("image release-version"))
            .collect();
        assert_eq!(checks.len(), 1, "{name}");
        assert!(
            checks[0].contains("--version \"$VERSION\"")
                && checks[0].contains("--bin=")
                && checks[0].contains("--root "),
            "{name}: {}",
            checks[0]
        );
        assert!(
            !version.to_string().contains("secrets.")
                && !version.to_string().contains("login-action"),
            "{name}: the job that reads the checked-out manifests holds no credential"
        );
        let promote = &workflow["jobs"][format!("image-{image}-promote")];
        assert_eq!(
            promote["needs"],
            json!([
                format!("image-{image}-authority"),
                format!("image-{image}-version")
            ]),
            "{name}"
        );
    }
    let workspace = at_helper("image-workspace", "0.1.117")?;
    let run = runs(&workspace["jobs"]["image-multi-bin-svc-version"])
        .into_iter()
        .find(|run| run.contains("image release-version"))
        .ok_or("no check")?
        .to_owned();
    assert!(run.contains("--root backend-rust"), "{run}");
    assert!(
        run.contains("--bin=legacy-server") && run.contains("--bin=legacy-migration"),
        "{run}"
    );
    Ok(())
}

#[test]
fn an_older_helper_pin_renders_the_release_path_as_before() -> TestResult {
    for name in SAMPLES {
        let workflow = at_helper(name, "0.1.116")?;
        let image = name_of(name);
        assert!(
            jobs(&workflow)?.keys().all(|id| !id.ends_with("-version")),
            "{name}"
        );
        assert_eq!(
            workflow["jobs"][format!("image-{image}-promote")]["needs"],
            json!([format!("image-{image}-authority")]),
            "{name}"
        );
        assert!(!workflow.to_string().contains("release-version"), "{name}");
    }
    Ok(())
}
