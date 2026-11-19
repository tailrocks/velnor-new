// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod restore;

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
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

pub use restore::RestoreOnly;

enum Listener {
    Job,
    Control(Arc<str>),
}

use super::env::{DockerCli, Environment};
use super::{Body, CacheServer, Config, api_stub, drive};
use crate::cli::{
    ApiStubArgs, CacheServerArgs, Cli, Command, ConformanceArgs, ControlArgs, UpArgs,
};
use crate::protocol::control_only;

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
    if matches!(
        command,
        Command::Up(_) | Command::Run(_) | Command::Build(_)
    ) {
        super::doctor::require_local(&DockerCli).await?;
    }
    match command {
        Command::CacheServer(args) => run_cache_server(args).await,
        Command::ApiStub(args) => run_api_stub(args).await,
        Command::Up(args) => up(args).await,
        Command::Down(args) => down(&args.dir).await,
        Command::Run(args) => drive::run(args).await,
        Command::Conformance(args) => conformance(args).await,
        Command::Build(args) => super::pair::build(args).await,
        Command::Doctor(args) => super::doctor::run(args).await,
        Command::Status(args) => super::status::run(&args.dir).await,
        Command::Jit(args) => super::pair::jit(args).await,
        Command::Supervise(args) => super::supervisor::serve(args).await,
    }
}

async fn conformance(args: ConformanceArgs) -> std::io::Result<()> {
    let mut logs = Vec::new();
    read_logs(&args.dir.join("logs"), "", &mut logs).await?;
    read_logs(&args.dir.join("logs/jobs"), "jobs/", &mut logs).await?;
    logs.sort();
    let stored = |probe: &str| {
        let text = std::fs::read_to_string(args.conformance.join(format!("{probe}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    };
    let (lines, reproduced) = crate::conformance::report(&logs, &stored);
    for line in &lines {
        println!("velnor-local: {line}");
    }
    if !reproduced {
        return Err(std::io::Error::other(
            "the run does not reproduce every stored probe result",
        ));
    }
    Ok(())
}

async fn read_logs(
    folder: &Path,
    prefix: &str,
    logs: &mut Vec<(String, String)>,
) -> std::io::Result<()> {
    let mut entries = match fs::read_dir(folder).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        read => read?,
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            logs.push((
                format!("{prefix}{name}"),
                fs::read_to_string(entry.path()).await?,
            ));
        }
    }
    Ok(())
}

async fn down(dir: &Path) -> std::io::Result<()> {
    let env = Environment::at(dir, DockerCli).await?;
    if let Some(stopped) = super::supervisor::stop(&env.dir).await? {
        println!("velnor-local: velnor-host stopped: {stopped}");
    }
    let [containers, volumes, networks] = env.down().await?;
    println!(
        "velnor-local: {} removed: {containers} containers, {volumes} volumes, {networks} networks",
        env.name
    );
    Ok(())
}

async fn up(args: UpArgs) -> std::io::Result<()> {
    let env = Environment::at(&args.environment.dir, DockerCli)
        .await?
        .with_labels(&args.environment.label);
    let repo = fs::canonicalize(&args.repo).await?;
    let pairs = super::supervisor::pairs(&args.environment).await?;
    println!("velnor-local: {}", runner_mode(pairs.as_ref()));
    let addresses = env.up(&repo, &args.environment).await?;
    let authority = match super::supervisor::start_with(&env.dir, pairs.as_ref(), &repo).await {
        Ok(authority) => authority,
        Err(error) => {
            env.down().await.ok();
            return Err(error);
        }
    };
    println!("velnor-local: authority on {}", authority.address);
    for pair in &authority.pairs {
        println!("velnor-local: pair {pair} listening");
    }
    println!(
        "velnor-local: {} is up, API stub at {}",
        env.name, addresses.stub_url
    );
    println!(
        "velnor-local: {}",
        crate::authority::warning(pairs.as_ref().is_some_and(|pairs| pairs.policy.is_some()))
    );
    Ok(())
}

pub(super) fn runner_mode(pairs: Option<&super::supervisor::Pairs>) -> String {
    pairs.map_or_else(
        || "runner mode plain: official runner containers, no job-start check".to_owned(),
        super::supervisor::Pairs::describe,
    )
}

async fn run_cache_server(args: CacheServerArgs) -> std::io::Result<()> {
    let config = Config {
        token_key: fs::read(&args.token_key).await?,
        dir: args.dir,
        upstream: args.upstream,
        limit_bytes: args.limit_bytes,
    };
    let restoring = args
        .restore_only
        .then(|| Arc::new(RestoreOnly::new(config.token_key.clone())));
    let server = CacheServer::open(config).await?;
    serve(
        "cache-server",
        args.listen,
        &args.control,
        move |request, listener| {
            let (server, restoring) = (Arc::clone(&server), restoring.clone());
            async move {
                let handle = |request| async move { server.handle(request).await };
                let restore = |request| async move {
                    match restoring {
                        Some(restoring) => restoring.handle(request, handle).await,
                        None => handle(request).await,
                    }
                };
                match listener {
                    Listener::Job => restore(request).await,
                    Listener::Control(token) => guarded(&token, request, restore).await,
                }
            }
        },
    )
    .await
}

async fn run_api_stub(args: ApiStubArgs) -> std::io::Result<()> {
    let stub = Arc::new(api_stub::ApiStub::new(api_stub::ApiStubConfig {
        repo: args.repo,
        dir: args.dir,
        cache_dir: args.cache_dir,
        default_branch: args.default_branch,
        diff_limit_bytes: args.diff_limit_bytes,
        local_setup: args.local_setup_velnor,
        job_cache_admin: args.job_cache_admin,
        ledger: args.ledger,
        token_key: match &args.token_key {
            Some(path) => Some(fs::read(path).await?),
            None => None,
        },
        standin: args.standin.map(|address| {
            Arc::new(api_stub::HttpStandIn::new(&address)) as Arc<dyn api_stub::StandIn>
        }),
    }));
    serve(
        "api-stub",
        args.listen,
        &args.control,
        move |request, listener| {
            let stub = Arc::clone(&stub);
            async move {
                match listener {
                    Listener::Job => stub.handle(request).await,
                    Listener::Control(token) => stub.handle_control(&token, request).await,
                }
            }
        },
    )
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

pub async fn open<H, F>(request: Request<Body>, handler: H) -> Response<Body>
where
    H: FnOnce(Request<Body>) -> F,
    F: Future<Output = Response<Body>>,
{
    if control_only(request.uri().path()) {
        let refusal = json!({"message": "this path is served only on the control port"});
        return super::json_response(403, &refusal);
    }
    handler(request).await
}

async fn serve<H, F>(
    name: &str,
    listen: SocketAddr,
    control: &ControlArgs,
    handler: H,
) -> std::io::Result<()>
where
    H: Fn(Request<Body>, Listener) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = Response<Body>> + Send + 'static,
{
    if let (Some(address), Some(path)) = (control.control_listen, &control.control_token) {
        let token: Arc<str> = fs::read_to_string(path).await?.trim().into();
        let inner = handler.clone();
        let control_handler = move |request| {
            let (token, inner) = (Arc::clone(&token), inner.clone());
            async move { inner(request, Listener::Control(token)).await }
        };
        let control_name = format!("{name} control");
        tokio::spawn(async move {
            if let Err(error) = listen_on(&control_name, address, control_handler).await {
                eprintln!("velnor-local {control_name}: {error}");
            }
        });
        let open_handler = move |request| {
            let handler = handler.clone();
            open(request, move |request| handler(request, Listener::Job))
        };
        return listen_on(name, listen, open_handler).await;
    }
    listen_on(name, listen, move |request| handler(request, Listener::Job)).await
}

async fn listen_on<H, F>(name: &str, listen: SocketAddr, handler: H) -> std::io::Result<()>
where
    H: Fn(Request<Body>) -> F + Clone + Send + 'static,
    F: Future<Output = Response<Body>> + Send + 'static,
{
    let listener = TcpListener::bind(listen).await?;
    eprintln!("velnor-local {name} listening on {listen}");
    accept(listener, handler).await
}

pub(super) async fn accept<H, F>(listener: TcpListener, handler: H) -> std::io::Result<()>
where
    H: Fn(Request<Body>) -> F + Clone + Send + 'static,
    F: Future<Output = Response<Body>> + Send + 'static,
{
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
