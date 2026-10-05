use velnor_actions_contract::{SCALE_SET_NAME, SUPPORTED_TARGETS, ScaleSetSelector, VELNOR_LABEL};

use super::mise_cache_target;

#[test]
fn cache_target_follows_each_hosted_and_scale_set_runner() -> Result<(), crate::RenderError> {
    assert_eq!(
        mise_cache_target("ubuntu-26.04", "linux")?,
        SUPPORTED_TARGETS[0]
    );
    assert_eq!(
        mise_cache_target("macos-15", "mac-arm"),
        Ok("aarch64-apple-darwin")
    );
    assert_eq!(
        mise_cache_target("macos-15-intel", "mac-intel"),
        Ok("x86_64-apple-darwin")
    );
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(crate::RenderError::Contract)?;
    assert_eq!(
        mise_cache_target(&selector.token(), "scale-set"),
        Ok(SUPPORTED_TARGETS[0])
    );
    Ok(())
}
