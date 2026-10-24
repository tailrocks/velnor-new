pub(super) fn valid_normal_tools_key(value: &str) -> bool {
    let Some(identity) = value.strip_prefix("mise-tools-v2-payload-v3-") else {
        return false;
    };
    identity.len() == 64
        && identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
