use super::owned_cli_bundle;

#[test]
fn owned_bundle_accepts_uppercase_extension_and_rejects_near_names() -> Result<(), crate::MiseError>
{
    for name in ["scli.app", "scli.APP", "scli.ApP"] {
        let path = std::path::PathBuf::from("/owned")
            .join(name)
            .join("Contents/MacOS/scli");
        assert_eq!(owned_cli_bundle(&path)?, format!("/owned/{name}"));
    }
    assert!(
        owned_cli_bundle(std::path::Path::new(
            "/owned/scli.app-copy/Contents/MacOS/scli"
        ))
        .is_err()
    );
    Ok(())
}
