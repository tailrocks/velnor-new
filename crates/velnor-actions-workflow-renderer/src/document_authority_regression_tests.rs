use super::*;

fn authority_job(condition: Option<&str>, cache_mode: Option<CacheMode>) -> Job {
    Job {
        display_name: "Writer".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: velnor_actions_contract::JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: condition.map(str::to_owned),
        permissions: None,
        cache_mode,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps: Vec::new(),
    }
}

fn authority_mode(job: &Job) -> Option<Yaml> {
    let mut entries = Vec::new();
    append_job_options(&mut entries, job, false, &BTreeMap::new(), &BTreeMap::new())
        .expect("options render");
    entries
        .into_iter()
        .find(|(key, _)| key == "cache-mode")
        .map(|(_, value)| value)
}

#[test]
fn custom_expression_push_gated_writer_renders_literal_write() {
    let job = authority_job(
        Some("(affected) && github.event_name == 'push'"),
        Some(CacheMode::Expression(
            "${{ needs.setup.outputs.cache-mode }}".to_owned(),
        )),
    );
    assert_eq!(authority_mode(&job), Some(Yaml::Str("write".to_owned())));
}

#[test]
fn custom_expression_unprovable_writer_keeps_no_declaration() {
    let unconditioned = authority_job(
        None,
        Some(CacheMode::Expression(
            "${{ needs.setup.outputs.cache-mode }}".to_owned(),
        )),
    );
    assert_eq!(authority_mode(&unconditioned), None);
    let pr_capable = authority_job(
        Some("(affected)"),
        Some(CacheMode::Expression(
            "${{ needs.setup.outputs.cache-mode }}".to_owned(),
        )),
    );
    assert_eq!(authority_mode(&pr_capable), None);
}
