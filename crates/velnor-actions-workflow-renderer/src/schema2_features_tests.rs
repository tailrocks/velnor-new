use super::{service_job, service_probe};
use crate::schema2::RunnerSpec;
use crate::yaml::Yaml;

#[test]
fn service_probe_uses_the_runner_network_address() {
    let hosted = RunnerSpec::hosted("ubuntu-26.04").expect("hosted runner");
    let scale = RunnerSpec::scale_set(Yaml::str("linux-amd64"));
    let (hosted_id, hosted_job) = service_job("hosted", "Hosted", &hosted);
    let (scale_id, scale_job) = service_job("scale", "Scale Set", &scale);
    assert_eq!(hosted_id, "hosted");
    assert_eq!(scale_id, "scale");
    assert!(rendered_run(&hosted_job).contains("127.0.0.1/6379"));
    assert!(rendered_run(&scale_job).contains("redis/6379"));
    assert!(service_probe(hosted.service_host(true)).contains("redis/6379"));
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
