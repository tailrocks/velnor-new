use super::cancel_service_jobs;
use crate::schema2::RunnerSpec;
use crate::yaml::Yaml;

#[test]
fn cancellation_probe_uses_the_runner_network_address() {
    let hosted = RunnerSpec::hosted("ubuntu-26.04").expect("hosted runner");
    let scale = RunnerSpec::scale_set(Yaml::str("linux-amd64"));
    let jobs = cancel_service_jobs(&hosted, &scale);
    assert_eq!(jobs.len(), 2);
    assert!(rendered_run(&jobs[0].1).contains("127.0.0.1 6379"));
    assert!(rendered_run(&jobs[1].1).contains("redis 6379"));
}

fn rendered_run(job: &Yaml) -> &str {
    let Yaml::Map(fields) = job else {
        panic!("job map")
    };
    let Some((_, Yaml::Seq(steps))) = fields.iter().find(|(key, _)| key == "steps") else {
        panic!("steps")
    };
    let Some(Yaml::Map(fields)) = steps.first() else {
        panic!("run step")
    };
    let Some((_, Yaml::Str(run))) = fields.iter().find(|(key, _)| key == "run") else {
        panic!("run command")
    };
    run
}
