#[test]
fn fetch_step_carries_auth_without_request_file() -> Result<(), RenderError> {
    let text = final_text()?;
    let fetch = step_block(&text, "Download every expected matrix artifact");
    for want in [
        "fetch-reports-v1",
        "GH_REPO",
        "github.repository",
        "GH_TOKEN",
        "github.token",
    ] {
        assert!(fetch.contains(want), "missing {want}:\n{fetch}");
    }
    assert!(
        !fetch.contains("VELNOR_REQUEST_FILE"),
        "fetch takes no request file:\n{fetch}"
    );
    Ok(())
}
