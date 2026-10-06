//! Schema 2 selector validation.

use std::error::Error;

use velnor_actions_contract_config::config::{SCALE_SET_NAME, SCALE_SET_PROFILE_ID, VELNOR_LABEL};
use velnor_actions_contract_config::{ExecutionConfig, ScaleSetSelector};

#[test]
fn illegal_scale_set_label_rejected() -> Result<(), Box<dyn Error>> {
    let direct = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[
            VELNOR_LABEL.to_owned(),
            "ubuntu-26.04".to_owned(),
            SCALE_SET_NAME.to_owned(),
        ],
    );
    let Err(err) = direct else {
        return Err("hosted catalog label accepted on a scale-set selector".into());
    };
    assert!(
        err.to_string().contains("illegal_label:ubuntu-26.04"),
        "{err}"
    );
    let mut execution = ExecutionConfig::hosted_default("ubuntu-26.04")?;
    let profile = execution
        .profiles
        .get_mut(SCALE_SET_PROFILE_ID)
        .ok_or("missing scale-set profile")?;
    profile.labels = vec!["ubuntu-26.04".to_owned(), VELNOR_LABEL.to_owned()];
    assert!(
        execution.validate("config.toml").is_err(),
        "config accepted a hosted catalog label on the scale set"
    );
    let weird = ScaleSetSelector::try_new("Bad", &[VELNOR_LABEL.to_owned(), "Bad".to_owned()]);
    assert!(weird.is_err(), "uppercase scale-set label accepted");
    Ok(())
}
