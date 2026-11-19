// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! One broken check per case, from the branches of the legacy script. The helper and the
//! legacy script must refuse every one of them, after the same requests. The cases are
//! written with `cases!`, one line each, which rustfmt leaves as written.

use serde_json::{Value, json};

use crate::admit_world::{AdmitWorld, MAIN, RUN, USER, tag};
use crate::preview_world::{SOURCE, TAG_OID};

pub(crate) type Case = (&'static str, fn(&mut AdmitWorld));

macro_rules! cases {
    ($($name:literal => |$w:ident| $body:expr;)*) => {
        &[$(($name, |$w: &mut AdmitWorld| { $body; })),*]
    };
}
pub(crate) use cases;

pub(crate) fn put(world: &mut AdmitWorld, suffix: &str, pointer: &str, value: Value) {
    world.edit(suffix, |v| {
        *v.pointer_mut(pointer)
            .unwrap_or_else(|| unreachable!("{suffix} {pointer}")) = value;
    });
}

pub(crate) fn push(world: &mut AdmitWorld, suffix: &str, pointer: &str, item: Value) {
    world.edit(suffix, |v| {
        if let Some(list) = v.pointer_mut(pointer).and_then(Value::as_array_mut) {
            list.push(item);
        }
    });
}

fn var(world: &mut AdmitWorld, key: &str, value: &str) {
    world.vars.insert(key.to_owned(), value.to_owned());
}

fn cargo(world: &mut AdmitWorld, manifest: &str) {
    let blob = crate::preview_world::lock_blob(manifest);
    put(
        world,
        &format!("/contents/Cargo.toml?ref={SOURCE}"),
        "/content",
        blob["content"].clone(),
    );
}

const CREATION: &str = "/rulesets/26696396";
const FROZEN: &str = "/rulesets/24397132";

fn manifest() -> String {
    format!("/contents/Cargo.toml?ref={SOURCE}")
}

fn run() -> String {
    format!("/actions/runs/{RUN}")
}

/// The environment of the run, the repository, the manifest and the owner.
pub(crate) const IDENTITY: &[Case] = cases! {
    "authority repository" => |w| var(w, "AUTHORITY_REPOSITORY", "evil/repo");
    "authority path" => |w| var(w, "AUTHORITY_PATH", ".github/workflows/other.yml");
    "authority sha is short" => |w| var(w, "AUTHORITY_SHA", "abc");
    "authority ref" => |w| var(w, "AUTHORITY_REF", "x");
    "run repository" => |w| var(w, "GITHUB_REPOSITORY", "evil/repo");
    "event is not a push" => |w| var(w, "GITHUB_EVENT_NAME", "pull_request");
    "ref is not the tag" => |w| var(w, "GITHUB_REF", "refs/heads/main");
    "workflow ref" => |w| var(w, "GITHUB_WORKFLOW_REF", "x");
    "workflow sha" => |w| var(w, "GITHUB_WORKFLOW_SHA", MAIN);
    "actor id" => |w| var(w, "GITHUB_ACTOR_ID", "1");
    "triggering actor is not a login" => |w| var(w, "GITHUB_TRIGGERING_ACTOR", "mal--lory");
    "triggering actor is empty" => |w| var(w, "GITHUB_TRIGGERING_ACTOR", "");
    "triggering actor has a path" => |w| var(w, "GITHUB_TRIGGERING_ACTOR", "../x");
    "run id is not a number" => |w| var(w, "GITHUB_RUN_ID", "abc");
    "run id is zero" => |w| var(w, "GITHUB_RUN_ID", "0");
    "run attempt is not a number" => |w| var(w, "GITHUB_RUN_ATTEMPT", "x");
    "repository id" => |w| put(w, "", "/id", json!(1));
    "repository name" => |w| put(w, "", "/full_name", json!("evil/repo"));
    "default branch" => |w| put(w, "", "/default_branch", json!("dev"));
    "repository owner" => |w| put(w, "", "/owner/id", json!(1));
    "manifest path" => |w| put(w, &manifest(), "/path", json!("x"));
    "manifest encoding" => |w| put(w, &manifest(), "/encoding", json!("none"));
    "version has no workspace" => |w| cargo(w, "[package]\nversion = \"0.1.82\"\n");
    "version is a number" => |w| cargo(w, "[workspace.package]\nversion = 1\n");
    "version has two parts" => |w| cargo(w, "[workspace.package]\nversion = \"1.2\"\n");
    "version has a leading zero" => |w| cargo(w, "[workspace.package]\nversion = \"01.2.3\"\n");
    "version is not TOML" => |w| cargo(w, "[[[");
    "owner id" => |w| put(w, USER, "/id", json!(1));
    "owner login" => |w| put(w, USER, "/login", json!("mallory"));
};

/// The run of the tag push and the tag rulesets.
pub(crate) const RULES: &[Case] = cases! {
    "run id" => |w| put(w, &run(), "/id", json!(1));
    "run attempt is below the run's" => |w| var(w, "GITHUB_RUN_ATTEMPT", "2");
    "run attempt is zero" => |w| put(w, &run(), "/run_attempt", json!(0));
    "run commit" => |w| put(w, &run(), "/head_sha", json!(MAIN));
    "run branch" => |w| put(w, &run(), "/head_branch", json!("main"));
    "run event" => |w| put(w, &run(), "/event", json!("workflow_dispatch"));
    "run workflow" => |w| put(w, &run(), "/path", json!(".github/workflows/ci.yml@x"));
    "run repository name" => |w| put(w, &run(), "/head_repository/full_name", json!("evil/repo"));
    "run repository id" => |w| put(w, &run(), "/head_repository/id", json!(1));
    "run creation time is not a time" => |w| put(w, &run(), "/created_at", json!("yesterday"));
    "run was created after the precheck expired" => |w| put(w, &run(), "/created_at", json!("2026-11-18T12:15:01Z"));
    "run was created before the precheck" => |w| put(w, &run(), "/created_at", json!("2026-11-18T11:59:59Z"));
    "creation ruleset is for branches" => |w| put(w, CREATION, "/target", json!("branch"));
    "creation ruleset is disabled" => |w| put(w, CREATION, "/enforcement", json!("disabled"));
    "creation ruleset id" => |w| put(w, CREATION, "/id", json!(1));
    "creation ruleset has no bypass actor" => |w| put(w, CREATION, "/bypass_actors", json!([]));
    "creation ruleset bypass actor" => |w| put(w, CREATION, "/bypass_actors/0/actor_id", json!(1));
    "creation ruleset bypass mode" => |w| put(w, CREATION, "/bypass_actors/0/bypass_mode", json!("pull_request"));
    "creation ruleset pattern" => |w| put(w, CREATION, "/conditions/ref_name/include", json!(["refs/tags/*"]));
    "creation ruleset excludes" => |w| put(w, CREATION, "/conditions/ref_name/exclude", json!(["x"]));
    "creation ruleset has another rule" => |w| put(w, CREATION, "/rules", json!([{"type": "creation"}, {"type": "update"}]));
    "creation ruleset has no rule" => |w| put(w, CREATION, "/rules", json!([]));
    "frozen ruleset is for branches" => |w| put(w, FROZEN, "/target", json!("branch"));
    "frozen ruleset is disabled" => |w| put(w, FROZEN, "/enforcement", json!("evaluate"));
    "frozen ruleset id" => |w| put(w, FROZEN, "/id", json!(1));
    "frozen ruleset has a bypass actor" => |w| put(w, FROZEN, "/bypass_actors", json!([{"actor_id": 1}]));
    "frozen ruleset pattern" => |w| put(w, FROZEN, "/conditions/ref_name/include", json!(["refs/tags/sdk-preview-*"]));
    "frozen ruleset excludes" => |w| put(w, FROZEN, "/conditions/ref_name/exclude", json!(["refs/tags/x"]));
    "frozen ruleset allows deletion" => |w| put(w, FROZEN, "/rules", json!([{"type": "update"}]));
    "frozen ruleset allows updates" => |w| put(w, FROZEN, "/rules", json!([{"type": "deletion"}]));
    "tag ref name" => |w| put(w, &format!("/git/ref/tags/{}", tag()), "/ref", json!("refs/tags/x"));
    "tag is lightweight" => |w| put(w, &format!("/git/ref/tags/{}", tag()), "/object/type", json!("commit"));
    "tag object id" => |w| put(w, &format!("/git/ref/tags/{}", tag()), "/object/sha", json!("xyz"));
    "tag object sha" => |w| put(w, &format!("/git/tags/{TAG_OID}"), "/sha", json!(MAIN));
    "tag name" => |w| put(w, &format!("/git/tags/{TAG_OID}"), "/tag", json!("x"));
    "tag points at a tree" => |w| put(w, &format!("/git/tags/{TAG_OID}"), "/object/type", json!("tree"));
    "tag points at another commit" => |w| put(w, &format!("/git/tags/{TAG_OID}"), "/object/sha", json!(MAIN));
    "tag commit url" => |w| put(w, &format!("/git/tags/{TAG_OID}"), "/object/url", json!("https://example.com"));
};

pub(crate) fn all() -> Vec<Case> {
    [
        IDENTITY,
        RULES,
        crate::admit_cases_tags::TAGS,
        crate::admit_cases_runs::PRODUCERS,
        crate::admit_cases_runs::ARCHIVES,
    ]
    .concat()
}
