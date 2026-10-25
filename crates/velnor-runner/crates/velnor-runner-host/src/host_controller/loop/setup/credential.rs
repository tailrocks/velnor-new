use crate::error::HostError;

pub(crate) fn connection_pat(credential: &[u8]) -> Result<&str, HostError> {
    if credential.is_empty()
        || credential.len() > 4096
        || credential.iter().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(HostError::Keychain);
    }
    std::str::from_utf8(credential).map_err(|_| HostError::Keychain)
}

#[cfg(test)]
mod tests {
    use super::connection_pat;
    use crate::error::HostError;

    #[test]
    fn validates_connection_credential_bytes_without_copying() {
        let credential = b"ghp_example";
        let pat = connection_pat(credential).map_err(|error| error.to_string());
        assert_eq!(pat, Ok("ghp_example"));
        assert!(matches!(connection_pat(b""), Err(HostError::Keychain)));
        assert!(matches!(
            connection_pat(b"bad token"),
            Err(HostError::Keychain)
        ));
        assert!(matches!(connection_pat(&[0xff]), Err(HostError::Keychain)));
        assert!(matches!(
            connection_pat(&vec![b'a'; 4097]),
            Err(HostError::Keychain)
        ));
    }
}
