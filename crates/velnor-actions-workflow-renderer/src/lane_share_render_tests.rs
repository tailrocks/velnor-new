use std::collections::BTreeMap;

use super::share_lanes;
use super::tests::{ctx, echo_step, paired, render_jobs, workflow_ir};

const CAP: usize = 500_000;
const LOGICAL_JOBS: usize = 21;
const STEPS_PER_JOB: usize = 48;

fn positions(yaml: &str, needle: &str) -> Vec<usize> {
    yaml.match_indices(needle)
        .map(|(position, _)| position)
        .collect()
}

#[test]
fn shared_lanes_keep_ci_under_github_file_cap() {
    let payload = "a".repeat(400);
    let steps: Vec<_> = (0..STEPS_PER_JOB)
        .map(|index| echo_step(index, &payload))
        .collect();
    let jobs = paired(&steps);
    let ir = workflow_ir();
    let context = ctx();
    let unshared_lanes = super::LaneShare {
        jobs: jobs.clone(),
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        runtime_preludes: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::new(),
    };
    let unshared = render_jobs(&ir, &unshared_lanes, &context).expect("unshared");
    assert!(
        unshared.len() > CAP,
        "unshared render has {} bytes",
        unshared.len()
    );

    let shared = share_lanes(&jobs, &context).expect("share");
    let yaml = render_jobs(&ir, &shared, &context).expect("shared");
    assert!(yaml.len() <= CAP, "shared ci.yml has {} bytes", yaml.len());
    assert!(!yaml.contains(&payload));
    assert!(yaml.contains("runs-on: ubuntu-26.04"));
    assert!(yaml.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
    assert!(yaml.contains("uses: ./.github/actions/rust-0"));

    let checkouts = positions(&yaml, "uses: actions/checkout@");
    let calls = positions(&yaml, "uses: ./.github/actions/rust-");
    assert_eq!(checkouts.len(), LOGICAL_JOBS * 2);
    assert_eq!(calls.len(), LOGICAL_JOBS * 2);
    assert_eq!(
        yaml.matches("# zizmor: ignore[self-repository]").count(),
        LOGICAL_JOBS * 2
    );
    for (index, (checkout, call)) in checkouts.iter().zip(&calls).enumerate() {
        assert!(checkout < call, "job {index} calls before checkout");
        if let Some(next_checkout) = checkouts.get(index + 1) {
            assert!(call < next_checkout, "shared job sequence interleaved");
        }
    }
    assert_eq!(shared.files.len(), LOGICAL_JOBS);
    for file in &shared.files {
        assert!(
            file.bytes.len() <= CAP,
            "{} has {} bytes",
            file.path,
            file.bytes.len()
        );
        assert!(file.bytes.contains("shell: bash"), "{}", file.path);
        assert!(file.bytes.contains(&payload), "{}", file.path);
        assert!(file.path.ends_with("/action.yml"), "{}", file.path);
    }
}
