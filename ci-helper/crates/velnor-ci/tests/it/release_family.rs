// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Mutex;

use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, Repo};
use velnor_ci::release_family::{Family, GitHubPost, family_tag, lines, tag_of, version_of};

const VERIFIED: &str = "1111111111111111111111111111111111111111";
const EARLIER: &str = "2222222222222222222222222222222222222222";

#[derive(Default)]
struct Fake {
    tag_on: Mutex<Option<String>>,
    published: bool,
    draft: Mutex<bool>,
    posts: Mutex<Vec<String>>,
}

impl GitHub for Fake {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let tag_on = self
            .tag_on
            .lock()
            .map_err(|e| CiError::new(e.to_string()))?
            .clone();
        let draft = *self.draft.lock().map_err(|e| CiError::new(e.to_string()))?;
        Ok(if path.contains("/releases?") {
            let mut releases = Vec::new();
            if self.published {
                releases.push(r#"{"tag_name":"v0.1.80","draft":false}"#);
            }
            if draft {
                releases.push(r#"{"tag_name":"v0.1.80","draft":true}"#);
            }
            format!("[{}]", releases.join(","))
        } else if path.contains("matching-refs") {
            match tag_on {
                Some(_) => r#"[{"ref":"refs/tags/v0.1.80"}]"#.to_owned(),
                None => "[]".to_owned(),
            }
        } else if path.contains("/git/ref/tags/") {
            format!(
                r#"{{"object":{{"type":"commit","sha":"{}"}}}}"#,
                tag_on.ok_or_else(|| CiError::new("no tag"))?
            )
        } else if path.contains("/compare/") {
            r#"{"status":"ahead"}"#.to_owned()
        } else {
            return Err(CiError::new(format!("unexpected {path}")));
        })
    }
}

impl GitHubPost for Fake {
    fn post(&self, path: &str, json: &str) -> Result<String, CiError> {
        if path.ends_with("/git/refs") {
            *self
                .tag_on
                .lock()
                .map_err(|e| CiError::new(e.to_string()))? = Some(VERIFIED.to_owned());
        } else {
            *self.draft.lock().map_err(|e| CiError::new(e.to_string()))? = true;
        }
        self.posts
            .lock()
            .map_err(|e| CiError::new(e.to_string()))?
            .push(json.to_owned());
        Ok("{}".to_owned())
    }
}

fn run(fake: &Fake) -> Result<Family, CiError> {
    family_tag(
        fake,
        &Repo::parse("tailrocks/velnor-new")?,
        VERIFIED,
        &tag_of("v{{ version }}", "0.1.80")?,
    )
}

#[test]
fn a_new_family_tag_is_created_on_the_verified_commit_with_one_draft_release() -> Result<(), CiError>
{
    let fake = Fake::default();
    assert_eq!(
        lines(&run(&fake)?),
        format!("tag=v0.1.80\ncommit={VERIFIED}\nbuild=true\n")
    );
    assert_eq!(
        fake.posts
            .lock()
            .map_err(|e| CiError::new(e.to_string()))?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn a_tag_on_another_commit_is_refused_before_anything_is_created() {
    let fake = Fake {
        tag_on: Mutex::new(Some(EARLIER.to_owned())),
        ..Fake::default()
    };
    assert!(run(&fake).is_err());
    assert!(fake.posts.lock().is_ok_and(|posts| posts.is_empty()));
}

#[test]
fn a_published_release_on_an_earlier_commit_is_no_release() -> Result<(), CiError> {
    let fake = Fake {
        tag_on: Mutex::new(Some(EARLIER.to_owned())),
        published: true,
        ..Fake::default()
    };
    assert_eq!(lines(&run(&fake)?), "tag=v0.1.80\nbuild=false\n");
    Ok(())
}

#[test]
fn the_binaries_must_share_one_version() {
    let metadata =
        |version: &str| format!(r#"{{"packages":[{{"name":"app","version":"{version}"}}]}}"#);
    let both = vec![
        metadata("0.1.80"),
        metadata("0.1.80").replace("app", "tools"),
    ];
    let names = vec!["app".to_owned(), "tools".to_owned()];
    assert_eq!(version_of(&both, &names).ok().as_deref(), Some("0.1.80"));
    let split = vec![
        metadata("0.1.80"),
        metadata("0.1.81").replace("app", "tools"),
    ];
    assert!(version_of(&split, &names).is_err());
}
