#[test]
fn nested_change_stays_narrow_with_root_package() -> TestResult {
    let repo = make_ws_repo(true)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(selects_both(&plan), "universe kept: {:?}", plan.task_ids);
    let beta = reasons_for(&plan, "beta");
    assert!(beta.iter().all(|r| *r == "affected_by_change"), "{beta:?}");
    let alpha = reasons_for(&plan, "alpha");
    assert!(alpha.iter().all(|r| *r == "forced_uncached"), "{alpha:?}");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}
