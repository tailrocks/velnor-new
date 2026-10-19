//! Shared assertions for each crate-job prelude.

use velnor_actions_contract::Job;

use super::names;

pub(super) fn assert_crate_preludes(jobs: &[(String, Job)]) {
    for (id, job) in jobs {
        assert!(!id.contains("velnor"), "unbranded {id}");
        let steps = names(job);
        let at = |name: &str| {
            steps
                .iter()
                .position(|seen| *seen == name)
                .expect("required prelude")
        };
        assert!(at("Checkout") < at("Prepare pinned tools"), "{steps:?}");
        assert!(
            at("Prepare pinned tools") < at("Prepare Rust components"),
            "{steps:?}"
        );
        assert_eq!(job.timeout_minutes.minutes(), 5);
    }
}
