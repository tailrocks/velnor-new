use bollard::errors::Error as DockerError;

use super::super::{classify_inspect, inspect_error};

#[test]
fn only_not_found_is_treated_as_absent() {
    assert_eq!(
        classify_inspect(Err(DockerError::DockerResponseServerError {
            status_code: 404,
            message: "missing runner-id".to_owned(),
        })),
        Ok(false)
    );
    assert_eq!(
        classify_inspect(Err(DockerError::DockerResponseServerError {
            status_code: 503,
            message: "temporary failure".to_owned(),
        })),
        Err(inspect_error(503))
    );
}

#[test]
fn running_state_must_be_present() -> Result<(), String> {
    for body in ["{}", r#"{"State":{}}"#] {
        let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
        assert_eq!(classify_inspect(Ok(info)), Err(inspect_error(200)));
    }
    Ok(())
}

#[test]
fn explicit_running_value_is_preserved() -> Result<(), String> {
    for (body, expected) in [
        (r#"{"State":{"Running":false}}"#, false),
        (r#"{"State":{"Running":true}}"#, true),
    ] {
        let info = serde_json::from_str(body).map_err(|error| error.to_string())?;
        assert_eq!(classify_inspect(Ok(info)), Ok(expected));
    }
    Ok(())
}
