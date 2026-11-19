// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Cases for the annotation of the tag, the ancestry and the tag moving.

use serde_json::{Value, json};

use crate::admit_cases::{Case, cases, put};
use crate::admit_world::{AdmitWorld, MAIN, tag};
use crate::preview_world::{SOURCE, TAG_OID};

fn annotated(suffix: &str) -> String {
    format!("{suffix}{TAG_OID}")
}

fn compare() -> String {
    format!("/compare/{SOURCE}...{MAIN}")
}

fn pretty(world: &mut AdmitWorld) {
    world.edit(&annotated("/git/tags/"), |t| {
        let record: Value =
            serde_json::from_str(t["message"].as_str().unwrap_or("")).unwrap_or_default();
        t["message"] = json!(serde_json::to_string_pretty(&record).unwrap_or_default());
    });
}

fn drop_key(world: &mut AdmitWorld, key: &str) {
    world.annotate(|r| {
        if let Some(map) = r.as_object_mut() {
            map.remove(key);
        }
    });
}

fn then(world: &mut AdmitWorld, suffix: &str, later: &Value) {
    world.edit(suffix, |v| *v = json!({"__seq": [v.clone(), later]}));
}

const FLOAT_SCHEMA: &str = "{\"artifact\":\"generator\",\"immutability\":{\"checked_at\":\"2026-11-18T12:00:00Z\",\"enabled\":true},\"repository\":\"tailrocks/velnor-new\",\"schema\":2.0,\"source_sha\":\"1111111111111111111111111111111111111111\",\"targets\":{},\"version\":\"0.1.82\"}";

pub(crate) const TAGS: &[Case] = cases! {
    "tag has no message" => |w| put(w, &annotated("/git/tags/"), "/message", json!(7));
    "annotation is too large" => |w| put(w, &annotated("/git/tags/"), "/message", json!(" ".repeat(9000)));
    "annotation is not canonical" => |w| pretty(w);
    "annotation has a duplicate key" => |w| put(w, &annotated("/git/tags/"), "/message", json!("{\"schema\":2,\"schema\":2}"));
    "annotation has an extra key" => |w| w.annotate(|r| r["extra"] = json!(1));
    "annotation lacks a key" => |w| drop_key(w, "version");
    "annotation schema" => |w| w.annotate(|r| r["schema"] = json!(1));
    "annotation schema is a float" => |w| put(w, &annotated("/git/tags/"), "/message", json!(FLOAT_SCHEMA));
    "annotation repository" => |w| w.annotate(|r| r["repository"] = json!("evil/repo"));
    "annotation artifact" => |w| w.annotate(|r| r["artifact"] = json!("runtime"));
    "annotation commit" => |w| w.annotate(|r| r["source_sha"] = json!(MAIN));
    "annotation version" => |w| w.annotate(|r| r["version"] = json!("9.9.9"));
    "annotation has a third target" => |w| w.annotate(|r| r["targets"]["windows"] = r["targets"]["linux"].clone());
    "annotation target has an extra field" => |w| w.annotate(|r| r["targets"]["linux"]["extra"] = json!(1));
    "annotation run id is zero" => |w| w.annotate(|r| r["targets"]["linux"]["producer_run_id"] = json!(0));
    "annotation attempt is negative" => |w| w.annotate(|r| r["targets"]["linux"]["producer_run_attempt"] = json!(-1));
    "annotation artifact ids are equal" => |w| w.annotate(|r| r["targets"]["darwin"]["payload_artifact_id"] = json!(11));
    "annotation digest is short" => |w| w.annotate(|r| r["targets"]["linux"]["payload_archive_sha256"] = json!("abc"));
    "annotation job id is a string" => |w| w.annotate(|r| r["targets"]["linux"]["qualification_job_id"] = json!("8101"));
    "precheck is off" => |w| w.annotate(|r| r["immutability"]["enabled"] = json!(false));
    "precheck is a string" => |w| w.annotate(|r| r["immutability"]["enabled"] = json!("true"));
    "precheck has an extra field" => |w| w.annotate(|r| r["immutability"]["by"] = json!("x"));
    "precheck time format" => |w| w.annotate(|r| r["immutability"]["checked_at"] = json!("2026-11-18T12:00:00+00:00"));
    "precheck time is not a date" => |w| w.annotate(|r| r["immutability"]["checked_at"] = json!("2026-02-30T12:00:00Z"));
    "precheck is after the tag" => |w| w.annotate(|r| r["immutability"]["checked_at"] = json!("2026-11-18T12:00:31Z"));
    "precheck is over a minute before the tag" => |w| w.annotate(|r| r["immutability"]["checked_at"] = json!("2026-11-18T11:59:29Z"));
    "tagger date is not UTC" => |w| put(w, &annotated("/git/tags/"), "/tagger/date", json!("2026-11-18T13:00:30+01:00"));
    "tagger date has no zone" => |w| put(w, &annotated("/git/tags/"), "/tagger/date", json!("2026-11-18T12:00:30"));
    "tagger date is missing" => |w| put(w, &annotated("/git/tags/"), "/tagger", json!({}));
    "main is a tag" => |w| put(w, "/git/ref/heads/main", "/object/type", json!("tag"));
    "main sha" => |w| put(w, "/git/ref/heads/main", "/object/sha", json!("xyz"));
    "source is behind main" => |w| put(w, &compare(), "/status", json!("behind"));
    "source diverged from main" => |w| put(w, &compare(), "/status", json!("diverged"));
    "merge base" => |w| put(w, &compare(), "/merge_base_commit/sha", json!(MAIN));
    "the tag moves during the download" => |w| then(w, &format!("/git/ref/tags/{}", tag()), &json!({"ref": format!("refs/tags/{}", tag()), "object": {"type": "tag", "sha": MAIN}}));
    "main moves during the download" => |w| then(w, &compare(), &json!({"status": "diverged", "merge_base_commit": {"sha": SOURCE}}));
};
