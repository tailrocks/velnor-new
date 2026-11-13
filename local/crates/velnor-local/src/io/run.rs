// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::fs;
use tokio::net::TcpListener;

use super::{Body, CacheServer, Config, api_stub};
use crate::cli::{ApiStubArgs, CacheServerArgs, Cli, Command};
use crate::store::Index;

#[must_use]
pub fn main() -> ExitCode {
    let command = Cli::parse().command;
    match tokio::runtime::Runtime::new().and_then(|runtime| runtime.block_on(run(command))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("velnor-local: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> std::io::Result<()> {
    match command {
        Command::CacheServer(args) => run_cache_server(args).await,
        Command::ApiStub(args) => run_api_stub(args).await,
    }
}

async fn run_cache_server(args: CacheServerArgs) -> std::io::Result<()> {
    let config = Config {
        token_key: fs::read(&args.token_key).await?,
        dir: args.dir,
        upstream: args.upstream,
        limit_bytes: args.limit_bytes,
    };
    let server = CacheServer::open(config).await?;
    serve("cache-server", args.listen, move |request| {
        let server = Arc::clone(&server);
        async move { server.handle(request).await }
    })
    .await
}

async fn run_api_stub(args: ApiStubArgs) -> std::io::Result<()> {
    let stub = Arc::new(api_stub::ApiStub::new(api_stub::ApiStubConfig {
        repo: args.repo,
        dir: args.dir,
        cache_dir: args.cache_dir,
    }));
    serve("api-stub", args.listen, move |request| {
        let stub = Arc::clone(&stub);
        async move { stub.handle(request).await }
    })
    .await
}

async fn serve<H, F>(name: &str, listen: SocketAddr, handler: H) -> std::io::Result<()>
where
    H: Fn(Request<Body>) -> F + Clone + Send + 'static,
    F: Future<Output = Response<Body>> + Send + 'static,
{
    let listener = TcpListener::bind(listen).await?;
    eprintln!("velnor-local {name} listening on {listen}");
    loop {
        let (stream, _) = listener.accept().await?;
        let handler = handler.clone();
        let service = service_fn(move |request: Request<Incoming>| {
            let handler = handler.clone();
            async move {
                let request =
                    request.map(|body| body.map_err(std::io::Error::other).boxed_unsync());
                Ok::<_, Infallible>(handler(request).await)
            }
        });
        tokio::spawn(async move {
            let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), service);
            if let Err(error) = connection.with_upgrades().await {
                eprintln!("connection: {error}");
            }
        });
    }
}

pub(super) async fn read_index(dir: &Path) -> std::io::Result<Index> {
    match fs::read(dir.join("index.json")).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Index::default()),
        Err(error) => Err(error),
    }
}

pub(super) async fn write_index(dir: &Path, index: &Index) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(index).map_err(std::io::Error::other)?;
    let staged = dir.join("index.json.tmp");
    fs::write(&staged, bytes).await?;
    fs::rename(&staged, dir.join("index.json")).await
}
