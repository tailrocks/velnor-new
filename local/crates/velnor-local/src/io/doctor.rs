// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use super::env::{DockerCli, Engine, program};
use super::supervisor::default_host;
use crate::cli::DoctorArgs;
use crate::doctor::{
    BUILD_FIX, PLAIN_IMAGES, Part, docker, healthy, host, pair_images, plain_images, render,
};
use crate::images::TARGETS;

async fn missing<'a, E: Engine>(engine: &E, tags: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut gone = Vec::new();
    for tag in tags {
        let found = engine
            .docker(&["image", "inspect", "--format", "{{.Id}}", tag])
            .await;
        if found.is_err() {
            gone.push(tag);
        }
    }
    gone
}

async fn host_part(explicit: Option<&Path>) -> Part {
    let fix = explicit.map_or_else(
        || BUILD_FIX.to_owned(),
        |path| {
            format!(
                "build or install velnor-host at {}, or omit --velnor-host",
                path.display()
            )
        },
    );
    let path = match explicit {
        Some(path) => std::path::absolute(path),
        None => match std::env::current_exe() {
            Ok(exe) => default_host(&exe).await,
            Err(error) => Err(error),
        },
    };
    let path = match path {
        Ok(path) => path,
        Err(error) => return host(Err(&error.to_string()), &fix),
    };
    let shown = path.display().to_string();
    if !path.is_file() {
        return host(Err(&format!("{shown} is not a file")), &fix);
    }
    match program(&shown, &["local", "--help"], None).await {
        Ok(_) => host(Ok(&shown), &fix),
        Err(_) => host(Err(&format!("{shown} was built without local mode")), &fix),
    }
}

pub async fn inspect<E: Engine>(engine: &E, velnor_host: Option<&Path>) -> Vec<Part> {
    let info = engine
        .docker(&["info", "--format", "{{.OperatingSystem}}|{{.Architecture}}"])
        .await;
    let socket = engine
        .docker(&[
            "context",
            "inspect",
            "--format",
            "{{.Endpoints.docker.Host}}",
        ])
        .await;
    let (plain, pair) = if info.is_ok() {
        (
            Some(missing(engine, PLAIN_IMAGES.iter().map(|(tag, _)| *tag)).await),
            Some(missing(engine, TARGETS.into_iter()).await),
        )
    } else {
        (None, None)
    };
    vec![
        docker(
            info.as_deref().map_err(|_| ()),
            socket.as_deref().map_err(|_| ()),
        ),
        plain_images(plain.as_deref()),
        pair_images(pair.as_deref()),
        host_part(velnor_host).await,
    ]
}

pub(super) async fn run(args: DoctorArgs) -> io::Result<()> {
    let parts = inspect(&DockerCli, args.velnor_host.as_deref()).await;
    print!("{}", render(&parts));
    if healthy(&parts) {
        return Ok(());
    }
    let count = parts.iter().filter(|part| part.fix.is_some()).count();
    Err(io::Error::other(format!("{count} part(s) missing")))
}
