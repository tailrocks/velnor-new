// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    io::{self, Read},
    path::Path,
};
use velnor_actions_contract::runner_publication_dockerfile_context;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();
    args.next().ok_or("missing executable name")?;
    let role = args.next().ok_or("missing role")?;
    let mode = args.next();
    if args.next().is_some() || mode.as_deref().is_some_and(|value| value != "--path") {
        return Err("unexpected arguments".into());
    }
    let context = runner_publication_dockerfile_context(&role).ok_or("unknown role")?;
    if mode.is_some() {
        println!("{}", context.dockerfile);
        return Ok(());
    }
    let mut bytes = Vec::new();
    io::stdin().take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err("Dockerfile too large".into());
    }
    let text = std::str::from_utf8(&bytes)?;
    context.analyze(text).map_err(|problem| problem.code())?;
    println!("{}", context.dockerfile);
    let directory = Path::new(context.dockerfile)
        .parent()
        .ok_or("Dockerfile parent missing")?;
    for source in context.tracked_sources {
        println!("{}", directory.join(source).display());
    }
    Ok(())
}
