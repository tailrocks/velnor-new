// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::io;

use zeroize::Zeroizing;

use super::transport::{Body, Request, Response, Transport};
use crate::error::CiError;
use crate::image_candidate::hashing::{Hashing, is_limit};
use crate::image_publish::challenge;
use crate::image_publish::digest::Digest;
use crate::image_publish::location::{self, DOWNLOAD_HOST, Kind, REGISTRY};

pub(super) const MAX_BODY: u64 = 2 * 1024 * 1024;
pub(super) const MANIFEST_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const BLOB_TYPE: &str = "application/octet-stream";

pub(super) fn fail<T>(message: &str) -> Result<T, CiError> {
    Err(CiError::new(message))
}

pub struct Registry<'a> {
    transport: &'a dyn Transport,
    token: Zeroizing<String>,
}

impl fmt::Debug for Registry<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Registry").finish_non_exhaustive()
    }
}

impl<'a> Registry<'a> {
    pub fn login(transport: &'a dyn Transport, actor: &str, secret: &str) -> Result<Self, CiError> {
        let challenged = send(
            transport,
            REGISTRY,
            "GET",
            location::api(),
            Vec::new(),
            Body::Empty,
        )?;
        let header = challenged.header("www-authenticate").map(str::to_owned);
        if challenged.status != 401 {
            return fail("Registry did not ask for authentication.");
        }
        challenge::verify(header.as_deref().unwrap_or_default())?;
        let basic = challenge::basic(actor, secret);
        let exchanged = send(
            transport,
            REGISTRY,
            "GET",
            challenge::token_url(),
            vec![
                ("Authorization", basic.as_str().to_owned()),
                ("Accept", "application/json".to_owned()),
            ],
            Body::Empty,
        )?;
        if exchanged.status != 200 {
            return fail("Registry token request failed.");
        }
        let token = challenge::token(&exchanged.bounded(MAX_BODY)?)?;
        Ok(Self { transport, token })
    }

    fn signed(
        &self,
        method: &'static str,
        url: String,
        mut headers: Vec<(&'static str, String)>,
        body: Body<'_>,
    ) -> Result<Response, CiError> {
        headers.insert(
            0,
            ("Authorization", format!("Bearer {}", self.token.as_str())),
        );
        send(self.transport, REGISTRY, method, url, headers, body)
    }

    pub(super) fn request(
        &self,
        method: &'static str,
        url: String,
        headers: Vec<(&'static str, String)>,
        body: Body<'_>,
    ) -> Result<Response, CiError> {
        let response = self.signed(method, url, headers, body)?;
        if (300..400).contains(&response.status) {
            return fail("Registry redirects are not accepted.");
        }
        Ok(response)
    }

    pub fn exists(&self, kind: Kind, digest: &Digest, size: u64) -> Result<bool, CiError> {
        let headers = match kind {
            Kind::Manifests => vec![("Accept", MANIFEST_TYPE.to_owned())],
            Kind::Blobs => Vec::new(),
        };
        let response =
            self.request("HEAD", location::object(kind, digest), headers, Body::Empty)?;
        if response.status == 404 {
            return Ok(false);
        }
        if response.status != 200
            || response.header("docker-content-digest") != Some(digest.as_str())
        {
            return fail("Registry digest HEAD verification failed.");
        }
        if response.header("content-length") != Some(size.to_string().as_str()) {
            return fail("Registry size HEAD verification failed.");
        }
        Ok(true)
    }

    pub fn verify(&self, kind: Kind, digest: &Digest, size: u64) -> Result<(), CiError> {
        let media = match kind {
            Kind::Manifests => MANIFEST_TYPE,
            Kind::Blobs => BLOB_TYPE,
        };
        let url = location::object(kind, digest);
        let mut response =
            self.signed("GET", url, vec![("Accept", media.to_owned())], Body::Empty)?;
        if kind == Kind::Blobs && matches!(response.status, 302 | 307) {
            response = self.download(&response, digest)?;
        }
        if (300..400).contains(&response.status) {
            return fail("Registry manifest redirects are not permitted.");
        }
        if response.status != 200 {
            return fail("Registry GET status is incorrect.");
        }
        if response.header("content-length") != Some(size.to_string().as_str()) {
            return fail("Registry GET size is different from the specified size.");
        }
        if response
            .header("docker-content-digest")
            .is_some_and(|header| header != digest.as_str())
        {
            return fail("Registry GET digest header is different from the requested digest.");
        }
        let mut hashing = Hashing::new(response.body, size);
        let copied = io::copy(&mut hashing, &mut io::sink());
        if copied.is_err_and(|error| !is_limit(&error)) {
            return fail("Registry GET request failed or timed out.");
        }
        let (sha, received) = hashing.finish();
        if received != size || sha != digest.hex() {
            return fail("Registry response bytes have an incorrect digest or size.");
        }
        Ok(())
    }

    fn download(&self, redirect: &Response, digest: &Digest) -> Result<Response, CiError> {
        let target = redirect.header("location").unwrap_or_default();
        let url = location::download(target, digest)?;
        send(
            self.transport,
            DOWNLOAD_HOST,
            "GET",
            url,
            vec![("Accept", BLOB_TYPE.to_owned())],
            Body::Empty,
        )
    }
}

fn send(
    transport: &dyn Transport,
    host: &str,
    method: &'static str,
    url: String,
    headers: Vec<(&'static str, String)>,
    body: Body<'_>,
) -> Result<Response, CiError> {
    location::fixed(&url, host)?;
    transport.send(Request {
        method,
        url,
        headers,
        body,
    })
}
