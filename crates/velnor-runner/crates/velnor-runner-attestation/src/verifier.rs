use std::collections::BTreeMap;
use std::error::Error;
use std::time::{Duration, Instant};

use crate::tuf_state::{RootResponseCapture, TufCache, TufRefreshRequest};
use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use der::{Decode, asn1::Utf8StringRef};
use futures_util::{TryStreamExt, stream};
use reqwest::{Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use sigstore_verify::{
    VerificationPolicy, Verifier,
    trust_root::TrustedRoot,
    types::{Bundle, Sha256Hash, SignatureContent},
};
use tough::{TargetName, Transport, TransportError, TransportErrorKind, TransportStream};
use url::Url;
use x509_cert::Certificate;

const MAX_BUNDLE_BYTES: usize = 2_000_000;
const MAX_CHECKSUM_BYTES: usize = 64 * 1024;
const MAX_TUF_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_REQUEST_BYTES: usize = 3 * 1024 * 1024;
pub(crate) const MAX_RESPONSE_BYTES: usize = 16 * 1024;
pub(crate) const VERIFY_DEADLINE: Duration = Duration::from_secs(90);
const SIGSTORE_TUF_ROOT: &[u8] = include_bytes!("sigstore-production-tuf-root.json");
const CI_OID_PREFIX: &str = "1.3.6.1.4.1.57264.1.";
const CI_OIDS: [&str; 23] = [
    "1.3.6.1.4.1.57264.1.1",
    "1.3.6.1.4.1.57264.1.2",
    "1.3.6.1.4.1.57264.1.3",
    "1.3.6.1.4.1.57264.1.4",
    "1.3.6.1.4.1.57264.1.5",
    "1.3.6.1.4.1.57264.1.6",
    "1.3.6.1.4.1.57264.1.8",
    "1.3.6.1.4.1.57264.1.9",
    "1.3.6.1.4.1.57264.1.10",
    "1.3.6.1.4.1.57264.1.11",
    "1.3.6.1.4.1.57264.1.12",
    "1.3.6.1.4.1.57264.1.13",
    "1.3.6.1.4.1.57264.1.14",
    "1.3.6.1.4.1.57264.1.15",
    "1.3.6.1.4.1.57264.1.16",
    "1.3.6.1.4.1.57264.1.17",
    "1.3.6.1.4.1.57264.1.18",
    "1.3.6.1.4.1.57264.1.19",
    "1.3.6.1.4.1.57264.1.20",
    "1.3.6.1.4.1.57264.1.21",
    "1.3.6.1.4.1.57264.1.22",
    "1.3.6.1.4.1.57264.1.23",
    "1.3.6.1.4.1.57264.1.24",
];

include!("verifier/transport.inc.rs");
include!("verifier/claims.inc.rs");
include!("verifier/inline.inc.rs");
include!("verifier/tests.inc.rs");
