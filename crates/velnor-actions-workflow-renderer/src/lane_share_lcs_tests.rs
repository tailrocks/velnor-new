use std::collections::BTreeMap;

use super::{
    CAP, HOSTED_RUNS, HOSTED_SUFFIX, Job, LOGICAL_JOBS, RenderError, SCALE_SUFFIX, Step, ctx,
    echo_step, heavy_steps, lane_job, mbx_restore, render_jobs, scale_token, share_lanes,
    workflow_ir,
};
use crate::cache_steps;

fn paired_jobs(hosted_steps: Vec<Step>, local_steps: Vec<Step>) -> BTreeMap<String, Job> {
    BTreeMap::from([
        (
            format!("rust-shift{HOSTED_SUFFIX}"),
            lane_job("hosted", HOSTED_RUNS, hosted_steps),
        ),
        (
            format!("rust-shift{SCALE_SUFFIX}"),
            lane_job("local", &scale_token(), local_steps),
        ),
    ])
}

#[test]
fn unsafe_logical_id_fails_closed() {
    let step = echo_step(0, "one");
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "rust.0__hosted".to_owned(),
        lane_job("hosted", HOSTED_RUNS, vec![step.clone()]),
    );
    jobs.insert(
        "rust.0__local".to_owned(),
        lane_job("local", &scale_token(), vec![step]),
    );
    let err = share_lanes(&jobs, &ctx()).expect_err("bad id");
    assert!(
        matches!(err, RenderError::InvalidWorkflow(ref problem) if problem == "bad_lane_id:rust.0__hosted"),
        "{err}"
    );
}

#[test]
fn unpaired_jobs_stay_inline() {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "actionlint".to_owned(),
        lane_job("actionlint", HOSTED_RUNS, vec![echo_step(0, "one")]),
    );
    let shared = share_lanes(&jobs, &ctx()).expect("share");
    assert!(shared.calls.is_empty());
    assert_eq!(shared.files.len(), 0);
    let kept = shared.jobs.get("actionlint").expect("actionlint");
    assert_eq!(kept.steps.len(), 1);
}

#[test]
fn shared_lanes_keep_ci_under_github_file_cap() {
    let payload = "a".repeat(400);
    let mut jobs = BTreeMap::new();
    for index in 0..LOGICAL_JOBS {
        let logical = format!("rust-{index}");
        jobs.insert(
            format!("{logical}{HOSTED_SUFFIX}"),
            lane_job(
                &format!("{logical} hosted"),
                HOSTED_RUNS,
                heavy_steps(&payload),
            ),
        );
        jobs.insert(
            format!("{logical}{SCALE_SUFFIX}"),
            lane_job(
                &format!("{logical} local"),
                &scale_token(),
                heavy_steps(&payload),
            ),
        );
    }
    let ir = workflow_ir();
    let context = ctx();
    let unshared = render_jobs(&ir, &jobs, &context, &BTreeMap::new()).expect("unshared");
    assert!(
        unshared.len() > CAP,
        "unshared render must exceed the GitHub cap, got {}",
        unshared.len()
    );
    let shared = share_lanes(&jobs, &context).expect("share");
    let yaml = render_jobs(&ir, &shared.jobs, &context, &shared.calls).expect("shared");
    assert!(
        yaml.len() <= CAP,
        "shared ci.yml must fit, got {}",
        yaml.len()
    );
    assert!(
        !yaml.contains(&payload),
        "shared ci.yml still inlines steps"
    );
    assert!(yaml.contains("runs-on: ubuntu-26.04"));
    assert!(yaml.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
    assert!(yaml.contains("uses: $/.github/actions/rust-0"));
    assert_eq!(shared.files.len(), LOGICAL_JOBS);
    for file in &shared.files {
        assert!(
            file.bytes.len() <= CAP,
            "{} is {} bytes",
            file.path,
            file.bytes.len()
        );
        assert!(file.bytes.contains("shell: bash"), "{}", file.path);
        assert!(file.bytes.contains(&payload), "{}", file.path);
        assert!(file.path.ends_with("/action.yml"), "{}", file.path);
    }
}

#[test]
fn lcs_shares_common_steps_when_mbx_counts_differ() {
    let before = echo_step(0, "before");
    let after = echo_step(1, "after");
    let mbx = mbx_restore();
    let jobs = paired_jobs(
        vec![before.clone(), mbx.clone(), after.clone()],
        vec![before, mbx.clone(), after, mbx],
    );

    let shared = share_lanes(&jobs, &ctx()).expect("share");
    let hosted = &shared.calls["rust-shift__hosted"];
    let local = &shared.calls["rust-shift__local"];
    assert_eq!(hosted.len(), 2);
    assert_eq!(local.len(), 2);
    assert_eq!(hosted[0].before_step, 0);
    assert_eq!(hosted[1].before_step, 2);
    assert_eq!(local[0].before_step, 0);
    assert_eq!(local[1].before_step, 2);
    assert!(
        shared
            .files
            .iter()
            .all(|file| !file.bytes.contains("Restore MBX objects"))
    );

    let mut jobs = shared.jobs;
    cache_steps::isolate_hosted_mbx_object_caches(&mut jobs);
    let yaml = render_jobs(&workflow_ir(), &jobs, &ctx(), &shared.calls).expect("render");
    assert_eq!(yaml.matches("name: Restore MBX objects").count(), 3);
    assert_eq!(yaml.matches("isolate-objects-cache: \"true\"").count(), 1);
}

#[test]
fn lcs_keeps_repeated_common_steps_in_order_across_shifted_mbx_boundaries() {
    let before = echo_step(0, "before");
    let repeated = echo_step(1, "repeated");
    let after = echo_step(2, "after");
    let mbx = mbx_restore();
    let jobs = paired_jobs(
        vec![
            before.clone(),
            mbx.clone(),
            repeated.clone(),
            repeated.clone(),
            after.clone(),
        ],
        vec![before, repeated.clone(), mbx, repeated, after],
    );

    let shared = share_lanes(&jobs, &ctx()).expect("share");
    let hosted = &shared.calls["rust-shift__hosted"];
    let local = &shared.calls["rust-shift__local"];
    assert_eq!(
        hosted
            .iter()
            .map(|call| call.before_step)
            .collect::<Vec<_>>(),
        [0, 2, 3]
    );
    assert_eq!(
        local
            .iter()
            .map(|call| call.before_step)
            .collect::<Vec<_>>(),
        [0, 1, 3]
    );
    let repeated_positions: Vec<_> = shared
        .files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| file.bytes.contains("name: echo 1").then_some(index))
        .collect();
    assert_eq!(repeated_positions.len(), 2);
    let combined = &shared.files[repeated_positions[1]].bytes;
    let repeated = combined.find("name: echo 1").expect("repeated step");
    let after = combined.find("name: echo 2").expect("tail step");
    assert!(
        repeated < after,
        "repeated step must stay before the tail step"
    );

    let mut jobs = shared.jobs;
    cache_steps::isolate_hosted_mbx_object_caches(&mut jobs);
    let yaml = render_jobs(&workflow_ir(), &jobs, &ctx(), &shared.calls).expect("render");
    let hosted_body = super::job_body(&yaml, "rust-shift__hosted");
    let local_body = super::job_body(&yaml, "rust-shift__local");
    let hosted_calls: Vec<_> = hosted_body
        .match_indices("name: Run shared steps")
        .map(|(at, _)| at)
        .collect();
    let local_calls: Vec<_> = local_body
        .match_indices("name: Run shared steps")
        .map(|(at, _)| at)
        .collect();
    let hosted_mbx = hosted_body
        .find("name: Restore MBX objects")
        .expect("hosted MBX");
    let local_mbx = local_body
        .find("name: Restore MBX objects")
        .expect("local MBX");
    assert_eq!(hosted_calls.len(), 3);
    assert_eq!(local_calls.len(), 3);
    assert!(hosted_calls[0] < hosted_mbx && hosted_mbx < hosted_calls[1]);
    assert!(local_calls[1] < local_mbx && local_mbx < local_calls[2]);
}

#[test]
fn lcs_tie_with_repeated_steps_keeps_ordered_source_indices() {
    let a = echo_step(0, "a");
    let b = echo_step(1, "b");
    let x = echo_step(2, "x");
    let hosted = [(4, &a), (8, &b), (13, &a)];
    let local = [(2, &b), (5, &a), (9, &b)];

    let matches = crate::lane_share_lcs::ordered_matches(&hosted, &local);

    assert_eq!(matches, [(8, 2), (13, 5)]);
    assert!(matches.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(matches.windows(2).all(|pair| pair[0].1 < pair[1].1));

    let repeated_hosted = [(4, &a), (8, &a)];
    let repeated_local = [(2, &a), (5, &b)];
    assert_eq!(
        crate::lane_share_lcs::ordered_matches(&repeated_hosted, &repeated_local),
        [(4, 2)]
    );

    let shifted_hosted = [(10, &a), (11, &x)];
    let shifted_local = [(20, &b), (21, &x), (22, &a)];
    assert_eq!(
        crate::lane_share_lcs::ordered_matches(&shifted_hosted, &shifted_local),
        [(11, 21)]
    );
}

fn reference_lcs_length(hosted: &[&Step], local: &[&Step]) -> usize {
    let mut lengths = vec![vec![0; local.len() + 1]; hosted.len() + 1];
    for (hosted_index, hosted_step) in hosted.iter().enumerate().rev() {
        for (local_index, local_step) in local.iter().enumerate().rev() {
            lengths[hosted_index][local_index] = if hosted_step == local_step {
                lengths[hosted_index + 1][local_index + 1] + 1
            } else {
                lengths[hosted_index + 1][local_index].max(lengths[hosted_index][local_index + 1])
            };
        }
    }
    lengths[0][0]
}

#[test]
fn lcs_returns_optimal_ordered_matches_for_small_repeated_ties() {
    let alphabet = [echo_step(0, "a"), echo_step(1, "b")];
    for hosted_len in 0_usize..=4 {
        for hosted_bits in 0..(1_usize << hosted_len) {
            let hosted: Vec<_> = (0..hosted_len)
                .map(|index| &alphabet[(hosted_bits >> index) & 1])
                .collect();
            let hosted_indexed: Vec<_> = hosted
                .iter()
                .enumerate()
                .map(|(index, step)| (index * 2, *step))
                .collect();
            for local_len in 0_usize..=4 {
                for local_bits in 0..(1_usize << local_len) {
                    let local: Vec<_> = (0..local_len)
                        .map(|index| &alphabet[(local_bits >> index) & 1])
                        .collect();
                    let local_indexed: Vec<_> = local
                        .iter()
                        .enumerate()
                        .map(|(index, step)| (index * 2, *step))
                        .collect();
                    let matches =
                        crate::lane_share_lcs::ordered_matches(&hosted_indexed, &local_indexed);

                    assert_eq!(matches.len(), reference_lcs_length(&hosted, &local));
                    assert!(matches.windows(2).all(|pair| pair[0].0 < pair[1].0));
                    assert!(matches.windows(2).all(|pair| pair[0].1 < pair[1].1));
                    for (hosted_source, local_source) in matches {
                        assert_eq!(hosted[hosted_source / 2], local[local_source / 2]);
                    }
                }
            }
        }
    }
}
