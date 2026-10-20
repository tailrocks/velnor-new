pub(super) const CURRENT_COLUMNS: [ColumnShape; 23] = [
    ColumnShape::new("id", "INTEGER", false, None),
    ColumnShape::new("kind", "TEXT", true, None),
    ColumnShape::new("subject", "TEXT", true, None),
    ColumnShape::new("state", "TEXT", true, None),
    ColumnShape::new("docker_id", "TEXT", false, None),
    ColumnShape::new("github_runner_id", "TEXT", false, None),
    ColumnShape::new("cleanup_proven", "INTEGER", true, Some("0")),
    ColumnShape::new("dind_id", "TEXT", false, None),
    ColumnShape::new("worker_volume", "TEXT", false, None),
    ColumnShape::new("scale_set_id", "INTEGER", false, None),
    ColumnShape::new("runner_request_id", "INTEGER", false, None),
    ColumnShape::new("runner_name", "TEXT", false, None),
    ColumnShape::new("acquire_attempted", "INTEGER", true, Some("0")),
    ColumnShape::new("acquire_resolved", "INTEGER", true, Some("0")),
    ColumnShape::new("acquired", "INTEGER", true, Some("0")),
    ColumnShape::new("jit_requested", "INTEGER", true, Some("0")),
    ColumnShape::new("docker_engine_id", "TEXT", false, None),
    ColumnShape::new("launch_phase", "TEXT", false, None),
    ColumnShape::new("launch_id", "TEXT", false, None),
    ColumnShape::new("assignment_key", "TEXT", false, None),
    ColumnShape::new("seed_generation_id", "TEXT", false, None),
    ColumnShape::new("runner_completed", "INTEGER", true, Some("0")),
    ColumnShape::new("worker_cleanup_proven", "INTEGER", true, Some("0")),
];

#[derive(Clone, Copy)]
pub(super) struct ColumnShape {
    pub(super) name: &'static str,
    pub(super) declared_type: &'static str,
    pub(super) not_null: bool,
    pub(super) default_value: Option<&'static str>,
}

impl ColumnShape {
    const fn new(
        name: &'static str,
        declared_type: &'static str,
        not_null: bool,
        default_value: Option<&'static str>,
    ) -> Self {
        Self {
            name,
            declared_type,
            not_null,
            default_value,
        }
    }
}
