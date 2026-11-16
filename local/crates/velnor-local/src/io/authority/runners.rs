// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request};
use tokio::fs;

use super::Backend;
use crate::io::env::{Engine, Environment};
use crate::io::pair::{MintError, Minted, mint_named};
use crate::io::proxy::Proxy;
use crate::side::Side;

#[derive(Debug)]
pub struct Runners<E> {
    env: Environment<E>,
    trusted: Proxy,
    untrusted: Option<Proxy>,
}

impl<E: Engine> Runners<E> {
    pub async fn open(env: Environment<E>) -> io::Result<Self> {
        let token = fs::read_to_string(env.dir.join("keys/control")).await?;
        let trusted = Proxy::authorized(&env.published("cache").await?, &token);
        let untrusted = env
            .published_if_present("fork-cache")
            .await?
            .map(|address| Proxy::authorized(&address, &token));
        Ok(Self {
            env,
            trusted,
            untrusted,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.env.dir
    }

    fn side(&self, side: Side) -> Option<&Proxy> {
        side.pick(&self.trusted, self.untrusted.as_ref()).ok()
    }

    async fn send(&self, side: Side, method: Method, path: &str) -> io::Result<Option<Vec<u8>>> {
        let Some(proxy) = self.side(side) else {
            return Ok(None);
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Empty::new().map_err(|never| match never {}).boxed_unsync())
            .map_err(io::Error::other)?;
        let response = proxy
            .forward(request)
            .await
            .map_err(|error| io::Error::other(error.msg))?;
        let status = response.status();
        let bytes = response.into_body().collect().await?.to_bytes();
        if !status.is_success() {
            return Err(io::Error::other(format!("{path} answered {status}")));
        }
        Ok(Some(bytes.to_vec()))
    }
}

impl<E: Engine + Send> Backend for Runners<E> {
    async fn get(&self, side: Side, path: &str) -> io::Result<Option<Vec<u8>>> {
        self.send(side, Method::GET, path).await
    }

    async fn delete(&self, side: Side, path: &str) -> io::Result<()> {
        self.send(side, Method::DELETE, path).await.map(|_| ())
    }

    async fn mint(&self, side: Side, name: Option<&str>) -> Result<Option<Minted>, MintError> {
        if self.side(side).is_none() {
            return Ok(None);
        }
        mint_named(&self.env, side, name).await.map(Some)
    }
}
