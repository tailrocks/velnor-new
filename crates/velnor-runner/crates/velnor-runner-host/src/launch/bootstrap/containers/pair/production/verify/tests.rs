use super::{
    effective_config_matches, has_no_published_ports, merge_environment, merge_labels,
    security_matches,
};
use std::collections::{BTreeMap, HashMap};

#[test]
fn verified_image_defaults_merge_with_exact_request_overrides() {
    let image_env = [
        "PATH=/usr/local/bin:/usr/bin".to_owned(),
        "MODE=image".to_owned(),
    ];
    let request_env = ["MODE=runner".to_owned()];
    let expected_env = BTreeMap::from([
        ("MODE".to_owned(), "runner".to_owned()),
        ("PATH".to_owned(), "/usr/local/bin:/usr/bin".to_owned()),
    ]);
    assert_eq!(
        merge_environment(Some(&image_env), Some(&request_env)).expect("valid environment"),
        expected_env
    );

    let image_labels = HashMap::from([
        (
            "org.opencontainers.image.version".to_owned(),
            "26.04".to_owned(),
        ),
        ("io.velnor.role".to_owned(), "image-value".to_owned()),
    ]);
    let request_labels = HashMap::from([("io.velnor.role".to_owned(), "runner".to_owned())]);
    let merged = merge_labels(Some(&image_labels), Some(&request_labels));
    assert_eq!(merged["org.opencontainers.image.version"], "26.04");
    assert_eq!(merged["io.velnor.role"], "runner");
    assert_ne!(
        merged,
        HashMap::from([("io.velnor.role".to_owned(), "runner".to_owned())])
    );
}

#[test]
fn environment_merge_rejects_malformed_and_unexpected_entries_remain_distinct() {
    assert!(merge_environment(Some(&["PATH".to_owned()]), None).is_err());
    assert!(
        merge_environment(
            Some(&["PATH=/wrong".to_owned(), "PATH=/expected".to_owned()]),
            None,
        )
        .is_err()
    );
    assert!(
        merge_environment(
            None,
            Some(&["PATH=/wrong".to_owned(), "PATH=/expected".to_owned()]),
        )
        .is_err()
    );
    assert_eq!(
        merge_environment(
            Some(&["PATH=/image".to_owned()]),
            Some(&["PATH=/request".to_owned()])
        )
        .expect("distinct image and request entries")["PATH"],
        "/request"
    );
    let image = ["PATH=/bin".to_owned()];
    let unexpected = ["PATH=/bin".to_owned(), "INJECTED=yes".to_owned()];
    assert_ne!(
        merge_environment(Some(&image), None).expect("image env"),
        merge_environment(Some(&unexpected), None).expect("actual env")
    );
}

#[test]
fn inspect_accepts_verified_image_defaults_and_rejects_unexpected_config() {
    let image = bollard::models::ImageConfig {
        env: Some(vec!["PATH=/usr/bin".to_owned()]),
        labels: Some(HashMap::from([(
            "org.opencontainers.image.version".to_owned(),
            "26.04".to_owned(),
        )])),
        ..Default::default()
    };
    let request = bollard::models::ContainerCreateBody {
        labels: Some(HashMap::from([(
            "io.velnor.role".to_owned(),
            "runner".to_owned(),
        )])),
        open_stdin: Some(true),
        stdin_once: Some(true),
        ..Default::default()
    };
    let actual = bollard::models::ContainerConfig {
        env: image.env.clone(),
        labels: Some(merge_labels(image.labels.as_ref(), request.labels.as_ref())),
        open_stdin: Some(true),
        stdin_once: Some(true),
        ..Default::default()
    };
    assert!(effective_config_matches(&actual, &image, &request).expect("valid config"));

    let mut extra_env = actual.clone();
    extra_env
        .env
        .as_mut()
        .expect("image environment")
        .push("UNEXPECTED=yes".to_owned());
    assert!(!effective_config_matches(&extra_env, &image, &request).expect("valid environment"));

    let mut duplicate_env = actual.clone();
    duplicate_env
        .env
        .as_mut()
        .expect("image environment")
        .push("PATH=/usr/bin".to_owned());
    assert!(effective_config_matches(&duplicate_env, &image, &request).is_err());

    let mut wrong_label = actual;
    wrong_label
        .labels
        .as_mut()
        .expect("labels")
        .insert("io.velnor.role".to_owned(), "dind".to_owned());
    assert!(!effective_config_matches(&wrong_label, &image, &request).expect("valid labels"));

    let mut extra_label = bollard::models::ContainerConfig {
        env: image.env.clone(),
        labels: Some(merge_labels(image.labels.as_ref(), request.labels.as_ref())),
        open_stdin: Some(true),
        stdin_once: Some(true),
        ..Default::default()
    };
    extra_label
        .labels
        .as_mut()
        .expect("labels")
        .insert("io.velnor.injected".to_owned(), "true".to_owned());
    assert!(!effective_config_matches(&extra_label, &image, &request).expect("valid labels"));
}

fn runner_capability_names() -> Vec<String> {
    [
        "CHOWN",
        "DAC_OVERRIDE",
        "FOWNER",
        "FSETID",
        "KILL",
        "SETGID",
        "SETUID",
        "SETPCAP",
        "NET_BIND_SERVICE",
        "NET_RAW",
        "SYS_CHROOT",
        "MKNOD",
        "AUDIT_WRITE",
        "SETFCAP",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn expected_runner_security() -> bollard::models::HostConfig {
    bollard::models::HostConfig {
        cap_drop: Some(vec!["ALL".to_owned()]),
        cap_add: Some(runner_capability_names()),
        security_opt: Some(vec![crate::worker::seccomp::runner_security_opt()]),
        ..Default::default()
    }
}

fn compact_runner_profile() -> String {
    let profile = crate::worker::seccomp::runner_security_opt();
    let json = serde_json::from_str::<serde_json::Value>(
        profile.strip_prefix("seccomp=").expect("seccomp prefix"),
    )
    .expect("pinned profile JSON");
    let compact_profile = format!(
        "seccomp={}",
        serde_json::to_string(&json).expect("compact profile JSON")
    );
    compact_profile
}

fn observed_runner_security() -> bollard::models::HostConfig {
    let cap_add = runner_capability_names()
        .into_iter()
        .rev()
        .map(|value| format!("CAP_{value}"))
        .collect();
    bollard::models::HostConfig {
        cap_drop: Some(vec!["CAP_ALL".to_owned()]),
        cap_add: Some(cap_add),
        security_opt: Some(vec![compact_runner_profile()]),
        ..Default::default()
    }
}

#[test]
fn observed_runner_capabilities_are_exact_sets_with_docker_prefix_normalization() {
    assert!(security_matches(
        &observed_runner_security(),
        &expected_runner_security()
    ));
}

#[test]
fn observed_runner_rejects_duplicate_extra_and_missing_capabilities() {
    let expected = expected_runner_security();
    let observed = observed_runner_security();
    let duplicate_cap = bollard::models::HostConfig {
        cap_add: Some({
            let mut caps = observed.cap_add.clone().expect("observed capabilities");
            caps.push("SETFCAP".to_owned());
            caps
        }),
        ..observed.clone()
    };
    assert!(!security_matches(&duplicate_cap, &expected));

    let extra_cap = bollard::models::HostConfig {
        cap_add: Some({
            let mut caps = runner_capability_names();
            caps.push("SYS_ADMIN".to_owned());
            caps
        }),
        ..observed.clone()
    };
    assert!(!security_matches(&extra_cap, &expected));

    let missing_cap = bollard::models::HostConfig {
        cap_add: Some(
            runner_capability_names()
                .into_iter()
                .filter(|capability| capability != "NET_RAW")
                .collect(),
        ),
        ..observed.clone()
    };
    assert!(!security_matches(&missing_cap, &expected));
}

#[test]
fn observed_runner_rejects_seccomp_drift_and_additional_security_options() {
    let expected = expected_runner_security();
    let observed = observed_runner_security();
    let unconfined = bollard::models::HostConfig {
        security_opt: Some(vec!["seccomp=unconfined".to_owned()]),
        ..observed.clone()
    };
    assert!(!security_matches(&unconfined, &expected));

    let extra_security = bollard::models::HostConfig {
        security_opt: Some(vec![
            compact_runner_profile(),
            "no-new-privileges".to_owned(),
        ]),
        ..observed
    };
    assert!(!security_matches(&extra_security, &expected));
}

#[test]
fn privileged_dind_has_no_runner_capability_or_seccomp_overrides() {
    let expected = bollard::models::HostConfig::default();
    let observed = bollard::models::HostConfig {
        cap_add: Some(Vec::new()),
        cap_drop: Some(Vec::new()),
        security_opt: Some(Vec::new()),
        ..Default::default()
    };
    assert!(security_matches(&observed, &expected));

    let runner_profile = bollard::models::HostConfig {
        security_opt: Some(vec![crate::worker::seccomp::runner_security_opt()]),
        ..Default::default()
    };
    assert!(!security_matches(&runner_profile, &expected));
}

#[test]
fn observed_pair_must_not_publish_host_ports() {
    assert!(has_no_published_ports(
        &bollard::models::HostConfig::default()
    ));
    assert!(has_no_published_ports(&bollard::models::HostConfig {
        publish_all_ports: Some(false),
        ..Default::default()
    }));
    assert!(!has_no_published_ports(&bollard::models::HostConfig {
        publish_all_ports: Some(true),
        ..Default::default()
    }));
    assert!(!has_no_published_ports(&bollard::models::HostConfig {
        port_bindings: Some(HashMap::from([("2375/tcp".to_owned(), None)])),
        ..Default::default()
    }));
}
