use super::*;

#[test]
fn tmpfs_driver_options_keep_exact_finite_byte_and_inode_limits() {
    for (bytes, inodes) in [(1_u64, 1_u64), (4096, 32), (12_000, 200)] {
        let options = tmpfs_options(bytes, inodes).expect("valid finite limits");
        assert_eq!(options.len(), 3);
        assert_eq!(options.get("type").map(String::as_str), Some("tmpfs"));
        assert_eq!(options.get("device").map(String::as_str), Some("tmpfs"));
        assert_eq!(
            options.get("o").map(String::as_str),
            Some(format!("size={bytes},nr_inodes={inodes},nosuid,nodev").as_str())
        );
    }
    assert!(tmpfs_options(0, 1).is_err());
    assert!(tmpfs_options(1, 0).is_err());
}

#[test]
fn producer_accepts_only_the_four_finite_tmpfs_storage_objects() {
    let supported = [
        StorageRole::SharedRun,
        StorageRole::RunnerHome,
        StorageRole::RunnerWork,
        StorageRole::DindData,
    ];
    for role in supported {
        assert!(tmpfs_role(role), "{} must use this producer", role.as_str());
        assert_eq!(
            role.contract().map(|contract| contract.medium),
            Some(crate::journal::StorageMedium::Tmpfs)
        );
    }
    assert!(!tmpfs_role(StorageRole::EngineMetadata));
    assert!(!tmpfs_role(StorageRole::RunnerRoot));
    assert!(!tmpfs_role(StorageRole::DindRoot));
}

#[test]
fn volume_response_requires_exact_local_driver_options_and_ownership_labels() {
    let options = tmpfs_options(4096, 32).expect("limits");
    let labels = HashMap::from([
        ("velnor.connection".to_owned(), "consumer".to_owned()),
        ("velnor.reservation".to_owned(), "17".to_owned()),
        ("velnor.operation".to_owned(), "19".to_owned()),
        ("velnor.role".to_owned(), "runner-home".to_owned()),
    ]);
    let volume = Volume {
        name: "owned-volume".to_owned(),
        driver: "local".to_owned(),
        options: options.clone(),
        labels: labels.clone(),
        scope: Some(VolumeScopeEnum::LOCAL),
        ..Default::default()
    };
    assert!(verify_volume(&volume, "owned-volume", &options, &labels).is_ok());

    let mut wrong = volume.clone();
    wrong.name = "foreign-volume".to_owned();
    assert!(verify_volume(&wrong, "owned-volume", &options, &labels).is_err());

    let mut wrong = volume.clone();
    wrong.driver = "nfs".to_owned();
    assert!(verify_volume(&wrong, "owned-volume", &options, &labels).is_err());

    let mut wrong = volume.clone();
    wrong.options.insert("o".to_owned(), "size=4096".to_owned());
    assert!(verify_volume(&wrong, "owned-volume", &options, &labels).is_err());

    let mut wrong = volume.clone();
    wrong.labels.remove("velnor.operation");
    assert!(verify_volume(&wrong, "owned-volume", &options, &labels).is_err());

    let mut wrong = volume;
    wrong.scope = Some(VolumeScopeEnum::GLOBAL);
    assert!(verify_volume(&wrong, "owned-volume", &options, &labels).is_err());
}
