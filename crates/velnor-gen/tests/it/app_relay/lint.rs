// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, Value, check_rules};

const SETUP: &str =
    "tailrocks/velnor-new/actions/setup-velnor@a87c35aed5b68dd3e82b36df3e8d7364311abba8";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn setup(version: &str) -> RenderedStep {
    RenderedStep {
        name: "Install velnor-ci".to_owned(),
        uses: Some(SETUP.to_owned()),
        with: vec![("version".to_owned(), Value::Text(version.to_owned()))],
        ..RenderedStep::default()
    }
}

fn checkout() -> RenderedStep {
    RenderedStep {
        name: "Check out".to_owned(),
        uses: Some(CHECKOUT.to_owned()),
        ..RenderedStep::default()
    }
}

fn mint() -> RenderedStep {
    RenderedStep {
        id: Some("app".to_owned()),
        name: "Create the release App token".to_owned(),
        env: vec![(
            "VELNOR_APP_PRIVATE_KEY".to_owned(),
            Value::Expr("${{ secrets.RELEASE_APP_PRIVATE_KEY }}".to_owned()),
        )],
        run: Some("velnor-ci app-token --owner=\"$OWNER\" --repository=\"$REPOSITORY\"".to_owned()),
        ..RenderedStep::default()
    }
}

fn job(id: &str, steps: Vec<RenderedStep>) -> RenderedJob {
    RenderedJob {
        id: id.to_owned(),
        runs_on: "ubuntu-latest".to_owned(),
        timeout_minutes: 10,
        steps,
        ..RenderedJob::default()
    }
}

fn rules_of(jobs: Vec<RenderedJob>) -> Vec<Rule> {
    check_rules(&RenderedWorkflow { jobs })
        .err()
        .into_iter()
        .flatten()
        .map(|violation| violation.rule)
        .collect()
}

#[test]
fn red_a_job_that_references_the_app_key_and_checks_out_the_repository_is_refused() {
    let red = job("tag", vec![setup("0.1.117"), checkout(), mint()]);
    assert_eq!(rules_of(vec![red]), [Rule::KeyJobRunsRepositoryCode]);
}

#[test]
fn green_the_key_job_runs_no_repository_code_and_the_building_job_holds_no_key() {
    let key_only = job("app-token-tag", vec![setup("0.1.117"), mint()]);
    let building = job("tag", vec![setup("0.1.117"), checkout()]);
    assert_eq!(rules_of(vec![building, key_only]), Vec::<Rule>::new());
}

#[test]
fn older_pins_may_still_keep_the_key_in_the_building_job() {
    let old = job("tag", vec![setup("0.1.116"), checkout(), mint()]);
    assert_eq!(rules_of(vec![old]), Vec::<Rule>::new());
}
