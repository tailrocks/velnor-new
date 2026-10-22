use std::collections::BTreeMap;

use super::{
    BUN_LEGS_ENV, BUN_MAX_PARALLEL_ENV, BunLeg, BunLegMatrix, attach_bun_matrices, bun_matrices_of,
};
use crate::yaml::Yaml;
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

fn leg(leg: u16, project: Option<&str>, shard: u16, total: u16) -> BunLeg {
    BunLeg {
        leg,
        project: project.map(str::to_owned),
        shard,
        total,
    }
}

fn projects(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn legs_are_projects_times_shards_in_order() {
    let matrix = BunLegMatrix::new(&projects(&["chromium", "webkit"]), 2, None);
    assert_eq!(
        matrix.legs,
        [
            leg(1, Some("chromium"), 1, 2),
            leg(2, Some("chromium"), 2, 2),
            leg(3, Some("webkit"), 1, 2),
            leg(4, Some("webkit"), 2, 2),
        ]
    );
    assert!(matrix.has_projects() && matrix.is_sharded());
    assert_eq!(
        matrix.marker(),
        "1/chromium/1/2,2/chromium/2/2,3/webkit/1/2,4/webkit/2/2"
    );
    let plain = BunLegMatrix::new(&[], 3, Some(2));
    assert_eq!(plain.marker(), "1//1/3,2//2/3,3//3/3");
    assert!(!plain.has_projects() && plain.is_sharded());
    let per_project = BunLegMatrix::new(&projects(&["a", "b"]), 1, None);
    assert!(per_project.has_projects() && !per_project.is_sharded());
    for matrix in [matrix, plain, per_project] {
        matrix.validate().expect("canonical");
    }
}

fn marked(legs: &str, cap: Option<&str>) -> BTreeMap<String, Job> {
    let mut env = BTreeMap::from([(BUN_LEGS_ENV.to_owned(), legs.to_owned())]);
    if let Some(cap) = cap {
        env.insert(BUN_MAX_PARALLEL_ENV.to_owned(), cap.to_owned());
    }
    let step = crate::shell_step("Test", vec!["true".to_owned()], env).expect("step");
    BTreeMap::from([("bun-a--browser".to_owned(), job(vec![step]))])
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Bun / a / browser".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps,
        check_runner: None,
    }
}

#[test]
fn markers_parse_back_to_the_canonical_matrix() {
    let matrix = BunLegMatrix::new(&projects(&["chromium", "webkit"]), 2, Some(3));
    let parsed = bun_matrices_of(&marked(&matrix.marker(), Some("3"))).expect("parse");
    assert_eq!(parsed.get("bun-a--browser"), Some(&matrix));
    assert!(bun_matrices_of(&BTreeMap::new()).expect("none").is_empty());
}

#[test]
fn malformed_or_non_canonical_markers_are_rejected() {
    let cases = [
        ("", None, "bun_matrix_bad_marker"),
        ("1/a/1", None, "bun_matrix_bad_marker"),
        ("x/a/1/1", None, "bun_matrix_bad_marker"),
        ("2//1/1", None, "bun_matrix_not_canonical"),
        ("1//1/2", None, "bun_matrix_not_canonical"),
        ("1//1/2,2//1/2", None, "bun_matrix_not_canonical"),
        ("1/b/1/1,2/a/1/1", None, "bun_matrix_projects_unsorted"),
        ("1/a/1/1,2//1/1", None, "bun_matrix_mixed_projects"),
        ("1/a b/1/1", None, "bun_matrix_bad_project"),
        ("1//1/1", Some("2"), "bun_matrix_bad_max_parallel"),
        ("1//1/1", Some("0"), "bun_matrix_bad_max_parallel"),
        ("1//0/0", None, "bun_matrix_not_canonical"),
    ];
    for (legs, cap, code) in cases {
        let error = bun_matrices_of(&marked(legs, cap)).expect_err(code);
        assert!(error.to_string().contains(code), "{legs}: {code}: {error}");
    }
    let mut partial = marked("1//1/1", Some("1"));
    if let Some(job) = partial.get_mut("bun-a--browser")
        && let StepKind::Shell { env, .. } = &mut job.steps[0].kind
    {
        env.remove(BUN_LEGS_ENV);
    }
    let error = bun_matrices_of(&partial).expect_err("partial");
    assert!(
        error.to_string().contains("bun_matrix_marker_partial"),
        "{error}"
    );
}

#[test]
fn strategy_is_include_legs_without_fail_fast_before_steps() {
    let mut document = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "bun-a--browser".to_owned(),
            Yaml::Map(vec![
                ("runs-on".to_owned(), Yaml::Str("ubuntu-26.04".to_owned())),
                ("steps".to_owned(), Yaml::Seq(Vec::new())),
            ]),
        )]),
    )]);
    let matrices = BTreeMap::from([(
        "bun-a--browser".to_owned(),
        BunLegMatrix::new(&projects(&["chromium"]), 2, Some(1)),
    )]);
    attach_bun_matrices(&mut document, &matrices).expect("attach");
    let Yaml::Map(entries) = &document else {
        panic!("document");
    };
    let Yaml::Map(jobs) = &entries[0].1 else {
        panic!("jobs");
    };
    let Yaml::Map(job) = &jobs[0].1 else {
        panic!("job");
    };
    let keys: Vec<&str> = job.iter().map(|entry| entry.0.as_str()).collect();
    assert_eq!(keys, ["runs-on", "strategy", "steps"]);
    let expected = Yaml::Map(vec![
        ("fail-fast".to_owned(), Yaml::Bool(false)),
        ("max-parallel".to_owned(), Yaml::Int(1)),
        (
            "matrix".to_owned(),
            Yaml::Map(vec![(
                "include".to_owned(),
                Yaml::Seq(vec![
                    Yaml::Map(vec![
                        ("leg".to_owned(), Yaml::Int(1)),
                        ("project".to_owned(), Yaml::Str("chromium".to_owned())),
                        ("shard".to_owned(), Yaml::Int(1)),
                        ("total".to_owned(), Yaml::Int(2)),
                    ]),
                    Yaml::Map(vec![
                        ("leg".to_owned(), Yaml::Int(2)),
                        ("project".to_owned(), Yaml::Str("chromium".to_owned())),
                        ("shard".to_owned(), Yaml::Int(2)),
                        ("total".to_owned(), Yaml::Int(2)),
                    ]),
                ]),
            )]),
        ),
    ]);
    assert_eq!(job[1].1, expected);
}
