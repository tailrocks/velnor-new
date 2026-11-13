// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod proxy;

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use clap::Parser;
use futures_util::TryStreamExt;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST, HeaderName};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;
use velnor_cache_rules::AlreadyExists;

use crate::cli::{CacheServerArgs, Cli, Command};
use crate::protocol::{
    CacheRequest, Error, PullRequestBase, Route, Upload, archive_url, block_list, parse, route,
    upload_kind,
};
use crate::store::{Index, Lookup};
use crate::token;

pub type Body = UnsyncBoxBody<Bytes, std::io::Error>;

const JSON_LIMIT_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub struct Config {
    pub dir: PathBuf,
    pub token_key: Vec<u8>,
    pub upstream: String,
    pub limit_bytes: u64,
}

#[derive(Debug)]
pub struct CacheServer {
    config: Config,
    proxy: proxy::Proxy,
    index: Mutex<Index>,
}

#[must_use]
pub fn main() -> ExitCode {
    let Cli {
        command: Command::CacheServer(args),
    } = Cli::parse();
    match tokio::runtime::Runtime::new().and_then(|runtime| runtime.block_on(run(args))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("velnor-local: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: CacheServerArgs) -> std::io::Result<()> {
    let config = Config {
        token_key: fs::read(&args.token_key).await?,
        dir: args.dir,
        upstream: args.upstream,
        limit_bytes: args.limit_bytes,
    };
    let server = CacheServer::open(config).await?;
    let listener = TcpListener::bind(args.listen).await?;
    eprintln!("velnor-local cache-server listening on {}", args.listen);
    loop {
        let (stream, _) = listener.accept().await?;
        let server = Arc::clone(&server);
        let service = service_fn(move |request: Request<Incoming>| {
            let server = Arc::clone(&server);
            async move {
                let request =
                    request.map(|body| body.map_err(std::io::Error::other).boxed_unsync());
                Ok::<_, Infallible>(server.handle(request).await)
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

impl CacheServer {
    pub async fn open(config: Config) -> std::io::Result<Arc<Self>> {
        fs::create_dir_all(config.dir.join("archives")).await?;
        fs::create_dir_all(config.dir.join("blocks")).await?;
        let index = match fs::read(config.dir.join("index.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Index::default(),
            Err(error) => return Err(error),
        };
        Ok(Arc::new(Self {
            proxy: proxy::Proxy::new(&config.upstream),
            config,
            index: Mutex::new(index),
        }))
    }

    pub async fn handle(&self, request: Request<Body>) -> Response<Body> {
        let method = request.method().clone();
        let uri = request.uri().clone();
        let path = uri.path();
        let result = match (&method, route(path)) {
            (&Method::POST, Route::Twirp(call)) => self.twirp(call, request).await,
            (&Method::PUT, Route::Archive(blob)) => self.upload(blob, request).await,
            (&Method::GET, Route::Archive(blob)) => self.download(blob).await,
            (&Method::PUT, Route::PullRequest(number)) => self.register(number, request).await,
            (_, Route::Upstream) => self.proxy.forward(request).await,
            _ => Err(Error::not_found(format!("{method} {path} is not served"))),
        };
        let response = result.unwrap_or_else(|error| json_response(error.status, &error.body()));
        eprintln!("{method} {uri} {}", response.status().as_u16());
        response
    }

    async fn twirp(&self, call: &str, request: Request<Body>) -> Result<Response<Body>, Error> {
        let host = header(&request, &HOST)?;
        let bearer = header(&request, &AUTHORIZATION)?;
        let token = bearer.strip_prefix("Bearer ").ok_or_else(|| {
            Error::unauthenticated("the Authorization header is not a bearer token")
        })?;
        let body: CacheRequest = parse(&read_small_body(request.into_body()).await?)?;
        let now = now();
        let mut index = self.index.lock().await;
        let claims =
            token::verify(token, &self.config.token_key, now).map_err(Error::unauthenticated)?;
        let access =
            token::access(&claims, &index.pull_request_bases).map_err(Error::precondition)?;
        let at = Lookup {
            repository: &claims.repository,
            access: &access,
            key: &body.key,
            version: &body.version,
        };
        let reply = match call {
            "GetCacheEntryDownloadURL" => match index.restore(&at, &body.restore_keys, now) {
                Some(record) => json!({
                    "ok": true,
                    "signed_download_url": archive_url(&host, &record.blob),
                    "matched_key": record.key,
                }),
                None => json!({"ok": false}),
            },
            "CreateCacheEntry" => match index.create(&at, random_blob()?, now) {
                Ok(record) => {
                    json!({"ok": true, "signed_upload_url": archive_url(&host, &record.blob)})
                }
                Err(AlreadyExists(existing)) => json!({
                    "ok": false,
                    "message": format!("the entry {} already exists in {}", existing.key, existing.scope.0),
                }),
            },
            "FinalizeCacheEntryUpload" => {
                self.finalize(&mut index, &at, &body.size_bytes, now)
                    .await?
            }
            _ => return Err(Error::not_found(format!("{call} is not served"))),
        };
        self.save(&index).await?;
        Ok(json_response(200, &reply))
    }

    async fn finalize(
        &self,
        index: &mut Index,
        at: &Lookup<'_>,
        size_bytes: &str,
        now: u64,
    ) -> Result<Value, Error> {
        let record = index
            .pending(at)
            .cloned()
            .ok_or_else(|| Error::not_found("no reservation for this key and version"))?;
        let size = fs::metadata(self.archive(&record.blob))
            .await
            .map_err(|_| Error::precondition("the archive was not uploaded"))?
            .len();
        if size_bytes != size.to_string() {
            return Err(Error::precondition(format!(
                "{size} bytes were uploaded, not {size_bytes}"
            )));
        }
        for gone in index.commit(record.id, size, now, self.config.limit_bytes) {
            match fs::remove_file(self.archive(&gone.blob)).await {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(Error::internal(error));
                }
                _ => {}
            }
        }
        Ok(json!({"ok": true, "entry_id": record.id.to_string()}))
    }

    async fn upload(&self, blob: &str, request: Request<Body>) -> Result<Response<Body>, Error> {
        if self.index.lock().await.blob(blob, false).is_none() {
            return Err(Error::not_found("no reservation for this upload"));
        }
        let blocks = self.config.dir.join("blocks").join(blob);
        match upload_kind(request.uri().query().unwrap_or_default())? {
            Upload::Whole => write_body(request.into_body(), &self.archive(blob)).await?,
            Upload::Block(id) => {
                fs::create_dir_all(&blocks).await.map_err(Error::internal)?;
                write_body(
                    request.into_body(),
                    &blocks.join(URL_SAFE_NO_PAD.encode(&id)),
                )
                .await?;
            }
            Upload::BlockList => {
                let xml = read_small_body(request.into_body()).await?;
                let assembled = fs::try_exists(self.archive(blob)).await.unwrap_or(false);
                if assembled && !fs::try_exists(&blocks).await.unwrap_or(true) {
                    return Ok(created());
                }
                let mut archive = File::create(self.archive(blob))
                    .await
                    .map_err(Error::internal)?;
                for id in block_list(&String::from_utf8_lossy(&xml))? {
                    let mut block = File::open(blocks.join(URL_SAFE_NO_PAD.encode(&id)))
                        .await
                        .map_err(|_| Error::invalid(format!("block {id} was not uploaded")))?;
                    tokio::io::copy(&mut block, &mut archive)
                        .await
                        .map_err(Error::internal)?;
                }
                archive.flush().await.map_err(Error::internal)?;
                fs::remove_dir_all(&blocks).await.map_err(Error::internal)?;
            }
        }
        Ok(created())
    }

    async fn download(&self, blob: &str) -> Result<Response<Body>, Error> {
        if self.index.lock().await.blob(blob, true).is_none() {
            return Err(Error::not_found("no cache entry at this address"));
        }
        let file = File::open(self.archive(blob))
            .await
            .map_err(Error::internal)?;
        let length = file.metadata().await.map_err(Error::internal)?.len();
        let body = StreamBody::new(ReaderStream::with_capacity(file, 1 << 20).map_ok(Frame::data));
        Response::builder()
            .header(CONTENT_LENGTH, length)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(body.boxed_unsync())
            .map_err(Error::internal)
    }

    async fn register(
        &self,
        number: &str,
        request: Request<Body>,
    ) -> Result<Response<Body>, Error> {
        let number: u64 = number
            .parse()
            .map_err(|_| Error::invalid(format!("{number} is not a pull request number")))?;
        let base: PullRequestBase = parse(&read_small_body(request.into_body()).await?)?;
        let mut index = self.index.lock().await;
        index.pull_request_bases.insert(number, base.base_branch);
        self.save(&index).await?;
        Ok(json_response(200, &json!({})))
    }

    fn archive(&self, blob: &str) -> PathBuf {
        self.config.dir.join("archives").join(blob)
    }

    async fn save(&self, index: &Index) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(index).map_err(Error::internal)?;
        let staged = self.config.dir.join("index.json.tmp");
        fs::write(&staged, bytes).await.map_err(Error::internal)?;
        fs::rename(&staged, self.config.dir.join("index.json"))
            .await
            .map_err(Error::internal)
    }
}

fn header(request: &Request<Body>, name: &HeaderName) -> Result<String, Error> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| Error::unauthenticated(format!("the {name} header is missing")))
}

async fn read_small_body(body: Body) -> Result<Bytes, Error> {
    Limited::new(body, JSON_LIMIT_BYTES)
        .collect()
        .await
        .map(http_body_util::Collected::to_bytes)
        .map_err(Error::invalid)
}

async fn write_body(mut body: Body, path: &Path) -> Result<(), Error> {
    let mut file = File::create(path).await.map_err(Error::internal)?;
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.map_err(Error::internal)?.into_data() {
            file.write_all(&data).await.map_err(Error::internal)?;
        }
    }
    file.flush().await.map_err(Error::internal)
}

fn random_blob() -> Result<String, Error> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("no system randomness"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn created() -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() = StatusCode::CREATED;
    response
}

fn json_response(status: u16, body: &Value) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::from(body.to_string()))
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}
