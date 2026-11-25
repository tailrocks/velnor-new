// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crypto_box::aead::rand_core::CryptoRngCore;
use crypto_box::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::CiError;

pub const FORMAT: u32 = 1;
const NAME_LIMIT: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub repository: String,
    pub run_id: u64,
    pub attempt: u64,
    pub job: String,
}

impl Binding {
    pub fn new(repository: &str, run_id: u64, attempt: u64, job: &str) -> Result<Self, CiError> {
        let plain = !job.is_empty()
            && job.len() <= NAME_LIMIT
            && job
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !plain {
            return Err(CiError::new(
                "--job is not a job identity of letters, digits, - and _",
            ));
        }
        Ok(Self {
            repository: repository.to_owned(),
            run_id,
            attempt,
            job: job.to_owned(),
        })
    }

    #[must_use]
    pub fn public_artifact(&self) -> String {
        format!(
            "velnor-app-key-{}-{}-{}",
            self.job, self.run_id, self.attempt
        )
    }

    #[must_use]
    pub fn token_artifact(&self) -> String {
        format!(
            "velnor-app-token-{}-{}-{}",
            self.job, self.run_id, self.attempt
        )
    }

    fn check(&self, expected: &Self) -> Result<(), CiError> {
        let differs = |field: &str| {
            CiError::new(format!(
                "the {field} of the handshake is not the {field} of this job"
            ))
        };
        if self.repository != expected.repository {
            return Err(differs("repository"));
        }
        if self.run_id != expected.run_id {
            return Err(differs("run id"));
        }
        if self.attempt != expected.attempt {
            return Err(differs("run attempt"));
        }
        if self.job != expected.job {
            return Err(differs("job identity"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublicKeyDocument {
    format: u32,
    #[serde(flatten)]
    binding: Binding,
    public_key: String,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: u32,
    #[serde(flatten)]
    binding: Binding,
    token: String,
}

pub fn generate(
    rng: &mut impl CryptoRngCore,
    binding: &Binding,
) -> (Zeroizing<[u8; 32]>, PublicKeyDocument) {
    let secret = SecretKey::generate(rng);
    let document = PublicKeyDocument {
        format: FORMAT,
        binding: binding.clone(),
        public_key: STANDARD.encode(secret.public_key().as_bytes()),
    };
    (Zeroizing::new(secret.to_bytes()), document)
}

impl PublicKeyDocument {
    pub fn to_json(&self) -> Result<String, CiError> {
        serde_json::to_string(self).map_err(|e| CiError::new(format!("cannot write the key: {e}")))
    }

    pub fn parse(bytes: &[u8], expected: &Binding) -> Result<Self, CiError> {
        let document: Self = serde_json::from_slice(bytes)
            .map_err(|e| CiError::new(format!("the public key artifact is unreadable: {e}")))?;
        if document.format != FORMAT {
            return Err(CiError::new(
                "the public key artifact has an unknown format",
            ));
        }
        document.binding.check(expected)?;
        Ok(document)
    }

    fn key(&self) -> Result<PublicKey, CiError> {
        STANDARD
            .decode(&self.public_key)
            .ok()
            .and_then(|bytes| PublicKey::from_slice(&bytes).ok())
            .ok_or_else(|| CiError::new("the public key is not 32 bytes of base64"))
    }
}

pub fn seal(
    rng: &mut impl CryptoRngCore,
    document: &PublicKeyDocument,
    token: &str,
) -> Result<String, CiError> {
    let envelope = Zeroizing::new(
        serde_json::to_vec(&Envelope {
            format: FORMAT,
            binding: document.binding.clone(),
            token: token.to_owned(),
        })
        .map_err(|e| CiError::new(format!("cannot encode the token: {e}")))?,
    );
    let sealed = document
        .key()?
        .seal(rng, &envelope)
        .map_err(|_| CiError::new("the token could not be sealed"))?;
    Ok(STANDARD.encode(sealed))
}

pub fn open(
    secret: &[u8; 32],
    sealed: &str,
    expected: &Binding,
) -> Result<Zeroizing<String>, CiError> {
    let refused = || CiError::new("the sealed token does not open with this job's key");
    let bytes = STANDARD.decode(sealed.trim()).map_err(|_| refused())?;
    let plain = Zeroizing::new(
        SecretKey::from_bytes(*secret)
            .unseal(&bytes)
            .map_err(|_| refused())?,
    );
    let envelope: Envelope = serde_json::from_slice(&plain).map_err(|_| refused())?;
    if envelope.format != FORMAT {
        return Err(refused());
    }
    envelope.binding.check(expected)?;
    Ok(Zeroizing::new(envelope.token))
}
