//! Archive-once plans, sorted inventories, interface allowlist (PAR-8.11/8.19).
use velnor_actions_mise::{
    ArchivePlan, MiseError, NextestArchive, NextestDriver, NextestList, NextestPartition,
    NextestRun, SortedInventory,
};

fn archive(
    driver: NextestDriver,
    package: &str,
    features: &[String],
    target: Option<&str>,
) -> Result<NextestArchive, String> {
    NextestArchive::new(driver, package, features, target).map_err(|err| err.to_string())
}

#[test]
fn archive_plan_runs_once_per_package_config() -> Result<(), String> {
    let mut plan = ArchivePlan::new();
    assert!(plan.is_empty());
    plan.add(&archive(NextestDriver::Cargo, "demo", &[], None)?)
        .map_err(|err| err.to_string())?;
    assert_eq!(plan.len(), 1);
    let repeat = plan.add(&archive(NextestDriver::Cargo, "demo", &[], None)?);
    let err = repeat.expect_err("repeated configuration must fail");
    assert!(
        matches!(err, MiseError::InvalidNextestInput { .. }),
        "typed rejection: {err}"
    );
    assert_eq!(plan.len(), 1);
    plan.add(&archive(
        NextestDriver::Cargo,
        "demo",
        &[],
        Some("aarch64-apple-darwin"),
    )?)
    .map_err(|err| err.to_string())?;
    plan.add(&archive(
        NextestDriver::Cargo,
        "demo",
        &["serde".to_owned()],
        None,
    )?)
    .map_err(|err| err.to_string())?;
    plan.add(&archive(NextestDriver::Cargo, "other", &[], None)?)
        .map_err(|err| err.to_string())?;
    assert_eq!(plan.len(), 4);
    Ok(())
}

#[test]
fn archive_plan_key_ignores_driver() -> Result<(), String> {
    let mut plan = ArchivePlan::new();
    plan.add(&archive(NextestDriver::Cargo, "demo", &[], None)?)
        .map_err(|err| err.to_string())?;
    let clash = plan.add(&archive(NextestDriver::Mbx, "demo", &[], None)?);
    assert!(
        matches!(clash, Err(MiseError::InvalidNextestInput { .. })),
        "both drivers share one archive file"
    );
    Ok(())
}

#[test]
fn sorted_inventory_sorts_and_dedups() {
    let inventory = SortedInventory::new(vec![
        "test_b".to_owned(),
        "test_a".to_owned(),
        "test_b".to_owned(),
    ]);
    assert_eq!(inventory.ids(), &["test_a".to_owned(), "test_b".to_owned()]);
}

#[test]
fn sorted_inventory_rejects_unsorted_or_duplicate() {
    let err = SortedInventory::from_sorted(vec!["test_b".to_owned(), "test_a".to_owned()])
        .expect_err("unsorted inventory must fail");
    assert!(
        matches!(err, MiseError::InvalidNextestInput { .. }),
        "typed rejection: {err}"
    );
    let err = SortedInventory::from_sorted(vec!["test_a".to_owned(), "test_a".to_owned()])
        .expect_err("duplicated inventory must fail");
    assert!(
        matches!(err, MiseError::InvalidNextestInput { .. }),
        "typed rejection: {err}"
    );
    let ok = SortedInventory::from_sorted(vec!["test_a".to_owned(), "test_b".to_owned()])
        .expect("sorted inventory must pass");
    assert_eq!(ok.ids().len(), 2);
}

#[test]
fn nextest_interfaces_are_only_archive_list_run() -> Result<(), String> {
    let partition = NextestPartition::new(1, 2).map_err(|err| err.to_string())?;
    let payloads = [
        archive(NextestDriver::Cargo, "demo", &[], None)?.payload(),
        NextestList::new(NextestDriver::Cargo, partition).payload(),
        NextestRun::new(NextestDriver::Cargo, partition, "m-abc", "p1")
            .map_err(|err| err.to_string())?
            .payload(),
    ];
    for payload in &payloads {
        assert_eq!(payload[0].to_string_lossy(), "cargo");
        assert_eq!(payload[1].to_string_lossy(), "nextest");
    }
    let verbs: Vec<String> = payloads
        .iter()
        .map(|payload| payload[2].to_string_lossy().into_owned())
        .collect();
    assert_eq!(verbs, vec!["archive", "list", "run"]);
    Ok(())
}
