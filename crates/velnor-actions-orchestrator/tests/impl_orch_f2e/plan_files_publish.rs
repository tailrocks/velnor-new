use super::*;

#[test]
fn plan_files_publish_locally_only() -> TestResult {
    let (_repo, response) = plan_response_for_source_change()?;
    let velnor_dir = TempDir::new()?;
    let dir = publish_plan_files(&response, velnor_dir.path())?;
    let mut files = Vec::new();
    let mut pending = vec![velnor_dir.path().to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path.strip_prefix(velnor_dir.path())?.to_path_buf());
            }
        }
    }
    files.sort();
    assert_eq!(
        files,
        [
            Path::new("local/matrix.json").to_path_buf(),
            Path::new("local/plan.json").to_path_buf()
        ]
    );
    assert_eq!(dir, velnor_dir.path().join("local"));
    Ok(())
}
