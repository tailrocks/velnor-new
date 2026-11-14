// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::io::{self, Read};

use velnor_runner_host::{HostError, SecretStore, read_secret, store_secret};

#[derive(Default)]
struct Keychain(RefCell<Vec<(String, String, Vec<u8>)>>);

impl SecretStore for Keychain {
    fn store(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), HostError> {
        self.0
            .borrow_mut()
            .push((service.into(), account.into(), secret.to_vec()));
        Ok(())
    }
}

#[test]
fn credential_import_stores_only_normalized_valid_token() -> Result<(), HostError> {
    let max = vec![b'x'; 4096];
    let mut max_lf = max.clone();
    max_lf.extend_from_slice(b"\n");
    let mut max_crlf = max.clone();
    max_crlf.extend_from_slice(b"\r\n");
    for (mut input, expected) in [
        (b"token".as_slice(), b"token".as_slice()),
        (b"token\n", b"token"),
        (b"token\r\n", b"token"),
        (max.as_slice(), max.as_slice()),
        (max_lf.as_slice(), max.as_slice()),
        (max_crlf.as_slice(), max.as_slice()),
    ] {
        let keychain = Keychain::default();
        let secret = read_secret(&mut input)?;
        store_secret(&keychain, "service", "account", &secret)?;
        assert_eq!(
            *keychain.0.borrow(),
            [("service".into(), "account".into(), expected.to_vec())]
        );
    }
    Ok(())
}

#[test]
fn credential_import_rejects_invalid_input_before_storage() {
    let long = vec![b'x'; 4097];
    let overflow = vec![b'x'; 4099];
    let mut trailing_lf = vec![b'x'; 4096];
    trailing_lf.extend_from_slice(b"\nZ");
    let mut trailing_crlf = vec![b'x'; 4096];
    trailing_crlf.extend_from_slice(b"\r\nZ");
    for mut input in [
        b"".as_slice(),
        b"\n",
        b" ",
        b"token value",
        b"token\0",
        b"token\r",
        b"token\n\n",
        "tøken".as_bytes(),
        long.as_slice(),
        overflow.as_slice(),
        trailing_lf.as_slice(),
        trailing_crlf.as_slice(),
    ] {
        let keychain = Keychain::default();
        let result = read_secret(&mut input)
            .and_then(|secret| store_secret(&keychain, "service", "account", &secret));
        assert!(result.is_err());
        assert!(keychain.0.borrow().is_empty());
    }
}

struct FailedRead;

impl Read for FailedRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("read failed"))
    }
}

#[test]
fn credential_import_propagates_reader_failure_without_storage() {
    let keychain = Keychain::default();
    let result = read_secret(&mut FailedRead)
        .and_then(|secret| store_secret(&keychain, "service", "account", &secret));
    assert!(result.is_err());
    assert!(keychain.0.borrow().is_empty());
}
