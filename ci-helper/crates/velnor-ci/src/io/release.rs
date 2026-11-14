// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;

use super::http::HttpGitHub;
use crate::cli::{PublishArgs, RecordArgs};
use crate::error::CiError;
use crate::github::Repo;
use crate::publish::{Request, SUMS, publish};
use crate::release::{PlainName, record};

pub(super) fn run_record(args: &RecordArgs) -> Result<String, CiError> {
    let outputs = args
        .outputs
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|text| (path.display().to_string(), text))
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))
        })
        .collect::<Result<Vec<_>, CiError>>()?;
    let lines = record(&outputs)?;
    super::append_outputs(&lines)?;
    Ok(lines)
}

pub(super) fn run_publish(args: &PublishArgs) -> Result<String, CiError> {
    let token = super::token()?;
    let repo = Repo::parse(&args.repo)?;
    let mut files = BTreeMap::new();
    for name in args.files.iter().map(PlainName::as_str).chain([SUMS]) {
        let path = args.dir.join(name);
        match fs::read(&path) {
            Ok(bytes) => {
                files.insert(name.to_owned(), bytes);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CiError::new(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        }
    }
    let request = Request {
        tag: args.tag.clone(),
        app: args.app.clone(),
        names: args.files.clone(),
        files,
    };
    publish(&HttpGitHub::new(super::api_url(), token), &repo, &request)
}
