// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::AUTHORIZATION;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::fs;
use tokio::net::TcpListener;

use super::env::{DockerCli, Environment};
use super::{Body, CacheServer, Config, api_stub, drive};
use crate::cli::{ApiStubArgs, CacheServerArgs, Cli, Command, ControlArgs, UpArgs};

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
        Command::Up(args) => up(args).await,
        Command::Down(args) => Environment::at(&args.dir, DockerCli).await?.down().await,
        Command::Run(args) => drive::run(args).await,
    }
}

async fn up(args: UpArgs) -> std::io::Result<()> {
    let env = Environment::at(&args.environment.dir, DockerCli).await?;
    let repo = fs::canonicalize(&args.repo).await?;
    let addresses = env.up(&repo, &args.environment).await?;
    println!(
        "velnor-local: {} is up, API stub at {}",
        env.name, addresses.stub_url
    );
    Ok(())
}

async fn run_cache_server(args: CacheServerArgs) -> std::io::Result<()> {
    let config = Config {
        token_key: fs::read(&args.token_key).await?,
        dir: args.dir,
        upstream: args.upstream,
        limit_bytes: args.limit_bytes,
    };
    let server = CacheServer::open(config).await?;
    serve("cache-server", args.listen, &args.control, move |request| {
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
        default_branch: args.default_branch,
        diff_limit_bytes: args.diff_limit_bytes,
        standin: args.standin.map(|address| {
            Arc::new(api_stub::HttpStandIn::new(&address)) as Arc<dyn api_stub::StandIn>
        }),
    }));
    serve("api-stub", args.listen, &args.control, move |request| {
        let stub = Arc::clone(&stub);
        async move { stub.handle(request).await }
    })
    .await
}

pub async fn guarded<H, F>(token: &str, request: Request<Body>, handler: H) -> Response<Body>
where
    H: FnOnce(Request<Body>) -> F,
    F: Future<Output = Response<Body>>,
{
    let expected = format!("Bearer {token}");
    let presented = request.headers().get(AUTHORIZATION);
    if presented.is_some_and(|value| value.as_bytes() == expected.as_bytes()) {
        return handler(request).await;
    }
    let refusal = json!({"message": "this port needs the control token as a bearer token"});
    super::json_response(401, &refusal)
}

async fn serve<H, F>(
    name: &str,
    listen: SocketAddr,
    control: &ControlArgs,
    handler: H,
) -> std::io::Result<()>
where
    H: Fn(Request<Body>) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = Response<Body>> + Send + 'static,
{
    if let (Some(address), Some(path)) = (control.control_listen, &control.control_token) {
        let token: Arc<str> = fs::read_to_string(path).await?.trim().into();
        let inner = handler.clone();
        let guarded_handler = move |request| {
            let (token, inner) = (Arc::clone(&token), inner.clone());
            async move { guarded(&token, request, inner).await }
        };
        let control_name = format!("{name} control");
        tokio::spawn(async move {
            if let Err(error) = listen_on(&control_name, address, guarded_handler).await {
                eprintln!("velnor-local {control_name}: {error}");
            }
        });
    }
    listen_on(name, listen, handler).await
}

async fn listen_on<H, F>(name: &str, listen: SocketAddr, handler: H) -> std::io::Result<()>
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
