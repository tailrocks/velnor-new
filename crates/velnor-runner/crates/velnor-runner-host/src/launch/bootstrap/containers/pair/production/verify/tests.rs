use super::{effective_config_matches, merge_environment, merge_labels};
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
