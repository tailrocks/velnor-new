#![forbid(unsafe_code)]
#![deny(unused_must_use)]

mod certificates;
mod client;
mod sdk;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        bail!("usage: velnor-testcontainers-tls-fixture <certs|sdk> <directory>");
    };
    let Some(directory) = arguments.next() else {
        bail!("usage: velnor-testcontainers-tls-fixture <certs|sdk> <directory>");
    };
    if arguments.next().is_some() {
        bail!("expected exactly one directory argument");
    }
    let directory = PathBuf::from(directory);

    match command.to_str() {
        Some("certs") => {
            certificates::generate_bundle(&directory).with_context(|| {
                format!(
                    "could not create certificate bundle in {}",
                    directory.display()
                )
            })?;
            println!("tls-certificate-bundle-ready");
        }
        Some("sdk") => sdk::run_qualification(&directory).await?,
        Some(_) | None => bail!("expected command `certs` or `sdk`"),
    }

    Ok(())
}

#[cfg(test)]
mod https_tests;
