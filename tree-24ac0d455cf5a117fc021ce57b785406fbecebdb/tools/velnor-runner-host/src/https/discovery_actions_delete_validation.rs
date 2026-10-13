use super::positive_canonical_u64;

pub(super) fn valid_scale_set_query(query: &str) -> bool {
    let Some(rest) = query.strip_prefix("api-version=6.0-preview&name=") else {
        return false;
    };
    let Some((name, id)) = rest.split_once("&runnerGroupId=") else {
        return false;
    };
    matches!(name, "ubuntu-24.04-scale-set" | "ubuntu-26.04-scale-set")
        && !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id
            .parse::<i64>()
            .is_ok_and(|value| value > 0 && value.to_string() == id)
}

pub(super) fn session_delete_path(path: &str) -> bool {
    let Some((prefix, session_id)) = path.rsplit_once("/sessions/") else {
        return false;
    };
    let Some(scale_set_id) = prefix.strip_prefix("_apis/runtime/runnerscalesets/") else {
        return false;
    };
    positive_canonical_u64(scale_set_id)
        && scale_set_id.parse::<i64>().is_ok()
        && safe_session_id_segment(session_id)
}

fn safe_session_id_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}
