//! Validate the query contract used by exact launch discovery.

use std::collections::HashMap;

use crate::journal::LaunchIdentity;

pub(super) struct RequestTarget {
    pub(super) method: &'static str,
    path: String,
    query_required: bool,
    expected_labels: Option<Vec<String>>,
}

impl RequestTarget {
    pub(super) fn get(path: &str, query_required: bool) -> Self {
        Self {
            method: "GET",
            path: path.to_owned(),
            query_required,
            expected_labels: None,
        }
    }

    pub(super) fn launch_list(identity: &LaunchIdentity) -> Self {
        Self {
            method: "GET",
            path: "/containers/json".to_owned(),
            query_required: true,
            expected_labels: Some(expected_labels(identity)),
        }
    }
}

pub(super) fn verify_versioned_path(actual: &str, expected: &RequestTarget) -> Result<(), String> {
    let versioned = actual
        .strip_prefix("/v")
        .ok_or_else(|| "Docker request path has no API version".to_owned())?;
    let slash = versioned
        .find('/')
        .ok_or_else(|| "Docker request API path is missing".to_owned())?;
    let version = &versioned[..slash];
    let Some((major, minor)) = version.split_once('.') else {
        return Err("Docker API version is malformed".to_owned());
    };
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("Docker API version is malformed".to_owned());
    }
    let path = &versioned[slash..];
    if expected.query_required {
        match path.split_once('?') {
            Some((actual_path, query)) if actual_path == expected.path && !query.is_empty() => {
                verify_query(query, expected)
            }
            _ => Err("Docker request path does not match the scripted request".to_owned()),
        }
    } else if path == expected.path {
        Ok(())
    } else {
        Err("Docker request path does not match the scripted request".to_owned())
    }
}

fn verify_query(query: &str, expected: &RequestTarget) -> Result<(), String> {
    let parameters = decode_query(query)?;
    if let Some(labels) = expected.expected_labels.as_ref() {
        // Bollard 0.21.1 serializes both bool fields and omits `limit` when it is `None`.
        if parameters.len() != 3
            || parameters.get("all").map(String::as_str) != Some("true")
            || parameters.get("size").map(String::as_str) != Some("false")
        {
            return Err("Docker container list options do not match the launch query".to_owned());
        }
        let filter_json = parameters
            .get("filters")
            .ok_or_else(|| "Docker container list filters are missing".to_owned())?;
        let filters: HashMap<String, Vec<String>> = serde_json::from_str(filter_json)
            .map_err(|_| "Docker container list filters are malformed".to_owned())?;
        if filters.len() != 1 || filters.get("label") != Some(labels) {
            return Err("Docker container list ownership filters do not match".to_owned());
        }
    }
    Ok(())
}

fn decode_query(query: &str) -> Result<HashMap<String, String>, String> {
    let mut values = HashMap::new();
    for pair in query.split('&') {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| "Docker query parameter is malformed".to_owned())?;
        if values
            .insert(decode_component(key)?, decode_component(value)?)
            .is_some()
        {
            return Err("Docker query parameter is duplicated".to_owned());
        }
    }
    Ok(values)
}

fn decode_component(encoded: &str) -> Result<String, String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                let high = bytes
                    .get(index + 1)
                    .and_then(|byte| hex_value(*byte))
                    .ok_or_else(|| "Docker query escape is malformed".to_owned())?;
                let low = bytes
                    .get(index + 2)
                    .and_then(|byte| hex_value(*byte))
                    .ok_or_else(|| "Docker query escape is malformed".to_owned())?;
                decoded.push(high * 16 + low);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| "Docker query is not UTF-8".to_owned())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn expected_labels(identity: &LaunchIdentity) -> Vec<String> {
    vec![
        "velnor.product=velnor".to_owned(),
        format!("velnor.instance={}", identity.instance_id()),
        format!("velnor.launch={}", identity.launch_id()),
        format!("velnor.engine={}", identity.engine_id()),
    ]
}

#[cfg(test)]
mod tests {
    use super::{RequestTarget, verify_versioned_path};
    use crate::journal::LaunchIdentity;

    #[test]
    fn launch_list_requires_all_states_and_exact_identity_filters() -> Result<(), String> {
        let identity = LaunchIdentity::new(
            "11111111111111111111111111111111",
            1,
            "22222222222222222222222222222222",
            "engine_identity",
        )
        .map_err(|error| error.to_string())?;
        let expected = RequestTarget::launch_list(&identity);
        let filters = format!(
            r#"{{"label":["velnor.product=velnor","velnor.instance={}","velnor.launch={}","velnor.engine={}"]}}"#,
            identity.instance_id(),
            identity.launch_id(),
            identity.engine_id()
        );
        let valid = format!("/v1.41/containers/json?all=true&size=false&filters={filters}");
        assert_eq!(verify_versioned_path(&valid, &expected), Ok(()));

        let missing_filters = "/v1.41/containers/json?all=true&size=false";
        assert!(verify_versioned_path(missing_filters, &expected).is_err());
        let malformed_query = "/v1.41/containers/json?all=true&size=false&filters=%GG";
        assert!(verify_versioned_path(malformed_query, &expected).is_err());
        let wrong_all = format!("/v1.41/containers/json?all=false&size=false&filters={filters}");
        assert!(verify_versioned_path(&wrong_all, &expected).is_err());
        let wrong_identity =
            filters.replace(identity.launch_id(), "33333333333333333333333333333333");
        let wrong_scope =
            format!("/v1.41/containers/json?all=true&size=false&filters={wrong_identity}");
        assert!(verify_versioned_path(&wrong_scope, &expected).is_err());
        let limited =
            format!("/v1.41/containers/json?all=true&size=false&filters={filters}&limit=1");
        assert!(verify_versioned_path(&limited, &expected).is_err());
        Ok(())
    }
}
