//! End-to-end `plan-v1` dispositions of container image build jobs.
//!
//! Fixture: the ten-crate affected-selection workspace plus two images.
//! `web` is a plain Dockerfile image whose context is `svc/web` and whose
//! only `COPY` sources are `app.sh` and `assets/`; `app` compiles package
//! `a1` before `BuildKit` (`cargo_bin`) and copies the staged binary.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{JobDisposition, Plan};

use crate::impl_common::TestResult;
use crate::impl_select::{commit, plan_event, plan_pr, plan_push};
use crate::impl_select_affected_jobs::{fixture, put};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const IMAGES: &str = "
[[images]]
id = \"web\"
repository = \"ghcr.io/acme/web\"
dockerfile = \"svc/web/Dockerfile\"
context = \"svc/web\"
platforms = [\"linux_amd64\"]

[[images]]
id = \"app\"
repository = \"ghcr.io/acme/app\"
dockerfile = \"apps/a1/Dockerfile\"
context = \".\"
platforms = [\"linux_amd64\"]
[images.prebuilt]
kind = \"cargo_bin\"
package = \"a1\"
bin = \"a1\"
stage = \"dist/a1\"
";

/// Fixture plus the image sources, committed as the base.
fn image_fixture() -> Result<(tempfile::TempDir, String), Box<dyn std::error::Error>> {
    let (dir, _) = fixture(IMAGES, false)?;
    let root = dir.path();
    put(root, "apps/a1/src/main.rs", "fn main() {}\n")?;
    put(
        root,
        "apps/a1/Dockerfile",
        &format!("FROM debian:12@{DIGEST}\nCOPY dist/a1 /usr/local/bin/a1\n"),
    )?;
    put(
        root,
        "svc/web/Dockerfile",
        &format!("FROM debian:12@{DIGEST}\nCOPY app.sh /bin/app.sh\nCOPY assets/ /assets/\n"),
    )?;
    put(root, "svc/web/app.sh", "#!/bin/sh\n")?;
    put(root, "svc/web/assets/logo.txt", "logo\n")?;
    put(root, "svc/web/README.md", "readme\n")?;
    let base = commit(root, "images")?;
    Ok((dir, base))
}

fn disposition(plan: &Plan, job: &str) -> Option<JobDisposition> {
    plan.jobs
        .iter()
        .find(|j| j.job_id == job)
        .map(|j| j.disposition)
}

fn runs(plan: &Plan) -> BTreeSet<&str> {
    plan.jobs
        .iter()
        .filter(|job| job.job_id.starts_with("image-") && job.disposition == JobDisposition::Run)
        .map(|job| job.job_id.as_str())
        .collect()
}

fn pr(edit: impl FnOnce(&Path) -> TestResult) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, base) = image_fixture()?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

#[test]
fn an_unrelated_crate_change_skips_both_images() -> TestResult {
    let plan = pr(|root| put(root, "apps/a2/src/lib.rs", "pub fn g() {}\n"))?;
    assert!(runs(&plan).is_empty(), "{:?}", plan.jobs);
    assert_eq!(
        disposition(&plan, "image-web-amd64"),
        Some(JobDisposition::NotAffected)
    );
    assert_eq!(
        disposition(&plan, "image-app-amd64"),
        Some(JobDisposition::NotAffected)
    );
    Ok(())
}

#[test]
fn the_prebuilt_package_and_its_dockerfile_select_the_cargo_bin_image() -> TestResult {
    let plan = pr(|root| put(root, "apps/a1/src/main.rs", "fn main() { let _ = 1; }\n"))?;
    assert_eq!(runs(&plan), BTreeSet::from(["image-app-amd64"]));
    let plan = pr(|root| {
        put(
            root,
            "apps/a1/Dockerfile",
            &format!("FROM debian:12@{DIGEST}\nCOPY dist/a1 /usr/local/bin/a1\n# edit\n"),
        )
    })?;
    assert_eq!(runs(&plan), BTreeSet::from(["image-app-amd64"]));
    Ok(())
}

#[test]
fn a_dependency_of_the_prebuilt_package_selects_the_image() -> TestResult {
    // a1 depends on core; a2 does not.
    let plan = pr(|root| put(root, "libs/core/src/lib.rs", "pub fn g() {}\n"))?;
    assert_eq!(runs(&plan), BTreeSet::from(["image-app-amd64"]));
    Ok(())
}

#[test]
fn only_copied_context_paths_select_a_plain_image_even_when_rust_broadens() -> TestResult {
    // Files outside every package are unowned for Rust, so every crate job
    // runs (hard decision 3); the plain image still follows its own claims.
    let readme = pr(|root| put(root, "svc/web/README.md", "changed\n"))?;
    assert_eq!(
        disposition(&readme, "image-web-amd64"),
        Some(JobDisposition::NotAffected)
    );
    for path in [
        "svc/web/app.sh",
        "svc/web/assets/logo.txt",
        "svc/web/Dockerfile",
    ] {
        let plan = pr(|root| put(root, path, "changed\n"))?;
        assert_eq!(
            disposition(&plan, "image-web-amd64"),
            Some(JobDisposition::Run),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn global_config_unknown_comparisons_and_full_events_run_every_image() -> TestResult {
    let all = BTreeSet::from(["image-app-amd64", "image-web-amd64"]);
    let config = pr(|root| put(root, ".velnor/notes.txt", "x\n"))?;
    assert_eq!(runs(&config), all, "{:?}", config.jobs);
    let (dir, base) = image_fixture()?;
    put(dir.path(), "apps/a2/src/lib.rs", "pub fn g() {}\n")?;
    let head = commit(dir.path(), "change")?;
    let push = plan_push(dir.path(), Some(&base), &head)?.0;
    assert_eq!(runs(&push), all);
    let merge_group = plan_event(dir.path(), Some(&base), &head, "merge_group")?.0;
    assert_eq!(runs(&merge_group), all);
    let no_base = plan_pr(dir.path(), None, &head)?.0;
    assert_eq!(runs(&no_base), all, "a missing base cannot skip work");
    Ok(())
}

#[test]
fn an_empty_change_set_skips_every_image() -> TestResult {
    let (dir, base) = image_fixture()?;
    let plan = plan_pr(dir.path(), Some(&base), &base)?.0;
    assert!(runs(&plan).is_empty(), "{:?}", plan.jobs);
    assert_eq!(
        disposition(&plan, "image-web-amd64"),
        Some(JobDisposition::NotAffected)
    );
    Ok(())
}
