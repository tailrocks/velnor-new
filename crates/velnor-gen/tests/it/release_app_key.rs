// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use serde_json::Value;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::release_family_fixture::{TOOLS_LIKE, generate};
use crate::release_fixture::{APP, LOCK, TOOLS, release_view};
use crate::release_unguarded::expected_release;

const KEY: &str = "${{ secrets.RELEASE_APP_PRIVATE_KEY }}";
const TOKEN: &str = "${{ steps.app.outputs.token }}";
const EVERY_SITE: &str =
    "roots = [\".\", \"tools\"]\npr_only_checks = [\"DCO\"]\n[[binaries]]\npackage = \"app\"\n";

type Mints = BTreeMap<String, (Value, Vec<Value>)>;

fn workflows(helper: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut view = release_view(&[APP, TOOLS], Some(EVERY_SITE))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", helper).into_bytes());
    let every_site = expected_release(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(vec![every_site, generate(helper, TOOLS_LIKE)?.0])
}

fn mints(helper: &str) -> Result<Mints, Box<dyn Error>> {
    let mut found = Mints::new();
    for (index, text) in workflows(helper)?.iter().enumerate() {
        let document = crate::yaml_tree::document(text)?;
        for (id, job) in document["jobs"].as_object().ok_or("no jobs")? {
            let steps = job["steps"].as_array().ok_or("a job without steps")?;
            let holders: Vec<&Value> = steps
                .iter()
                .filter(|step| step.to_string().contains(KEY))
                .collect();
            if let [holder] = holders[..] {
                found.insert(format!("{index}/{id}"), (holder.clone(), steps.clone()));
            } else {
                assert!(holders.is_empty(), "{id}: {holders:?}");
            }
        }
    }
    Ok(found)
}

fn runs_release_plz(step: &Value) -> bool {
    step["run"]
        .as_str()
        .is_some_and(|run| run.starts_with("\"$RUNNER_TEMP/release-plz\" "))
}

fn helper_permissions(run: &str) -> BTreeSet<String> {
    run.split_whitespace()
        .filter_map(|word| word.strip_prefix("--permission="))
        .map(str::to_owned)
        .collect()
}

fn action_permissions(step: &Value) -> BTreeSet<String> {
    step["with"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(input, access)| {
            let scope = input.strip_prefix("permission-")?.replace('-', "_");
            Some(format!("{scope}:{}", access.as_str()?))
        })
        .collect()
}

#[test]
fn from_helper_0_1_114_the_release_app_key_reaches_only_a_helper_mint_that_every_job_revokes()
-> TestResult {
    let actions = mints("0.1.113")?;
    let helpers = mints("0.1.114")?;
    assert_eq!(
        actions.keys().collect::<Vec<_>>(),
        [
            "0/finalize",
            "0/gate",
            "0/release-pr",
            "0/tag",
            "1/family",
            "1/release-pr",
            "1/tag"
        ]
    );
    assert_eq!(
        actions.keys().collect::<Vec<_>>(),
        helpers.keys().collect::<Vec<_>>()
    );
    for (job, (action, _)) in &actions {
        assert!(
            action["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/create-github-app-token@")),
            "{job}: helper 0.1.113 no longer renders the action: {action}"
        );
        let (mint, steps) = &helpers[job];
        assert!(
            mint["uses"].is_null() && mint["with"].is_null(),
            "{job}: {mint}"
        );
        let run = mint["run"].as_str().unwrap_or_default();
        assert!(
            run.starts_with("velnor-ci app-token --owner=\"$OWNER\" --repository=\"$REPOSITORY\" "),
            "{job}: {run}"
        );
        assert!(!run.contains("secrets."), "{job}: {run}");
        assert_eq!(mint["env"]["VELNOR_APP_PRIVATE_KEY"], KEY, "{job}");
        assert_eq!(
            mint["env"]["VELNOR_APP_CLIENT_ID"],
            action["with"]["client-id"]
        );
        assert_eq!(mint["env"]["OWNER"], action["with"]["owner"]);
        assert_eq!(mint["env"]["REPOSITORY"], action["with"]["repositories"]);
        assert_eq!(helper_permissions(run), action_permissions(action), "{job}");
        let index = steps
            .iter()
            .position(|step| step == mint)
            .unwrap_or(usize::MAX);
        let code = steps.iter().position(runs_release_plz);
        assert_eq!(
            code.is_some(),
            job.ends_with("/release-pr") || job.ends_with("/tag"),
            "{job}"
        );
        assert!(
            code.is_none_or(|code| index < code),
            "{job}: the key follows release-plz"
        );
        let last = steps.last().ok_or("no steps")?;
        assert_eq!(last["env"]["VELNOR_APP_TOKEN"], TOKEN, "{job}: {last}");
        assert_eq!(last["env"]["VELNOR_API_URL"], mint["env"]["VELNOR_API_URL"]);
        assert_eq!(
            last["if"], "${{ always() && steps.app.outputs.token != '' }}",
            "{job}: {last}"
        );
        assert_eq!(last["run"], "velnor-ci revoke-token", "{job}: {last}");
    }
    for text in workflows("0.1.114")? {
        assert!(
            !text.contains("create-github-app-token") && !text.contains("private-key"),
            "an action receives the release App key"
        );
    }
    Ok(())
}
