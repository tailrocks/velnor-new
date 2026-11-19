// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use velnor_local::io::env::Engine;

#[derive(Debug, Default)]
pub(crate) struct FakeDocker {
    pub(crate) labelled: Mutex<BTreeMap<String, String>>,
    pub(crate) calls: Mutex<Vec<Vec<String>>>,
}

impl FakeDocker {
    pub(crate) fn left(&self) -> Vec<String> {
        let labelled = self.labelled.lock().unwrap_or_else(|e| unreachable!("{e}"));
        labelled.keys().cloned().collect()
    }

    fn record(&self, args: &[&str]) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| unreachable!("{e}"));
        calls.push(args.iter().map(|arg| (*arg).to_owned()).collect());
    }

    pub(crate) fn started(&self) -> Vec<Vec<String>> {
        let calls = self.calls.lock().unwrap_or_else(|e| unreachable!("{e}"));
        calls
            .iter()
            .filter(|call| call.starts_with(&["run".to_owned(), "-d".to_owned()]))
            .cloned()
            .collect()
    }
}

pub(crate) fn after<'a>(call: &'a [String], flag: &str) -> Vec<&'a str> {
    call.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

impl Engine for &FakeDocker {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        self.record(args);
        std::future::ready(self.answer(args))
    }

    fn logged(&self, args: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        self.record(args);
        std::future::ready(Ok(true))
    }
}

impl FakeDocker {
    fn answer(&self, args: &[&str]) -> io::Result<String> {
        let mut labelled = self.labelled.lock().unwrap_or_else(|e| unreachable!("{e}"));
        let after = |flag: &str| {
            args.iter()
                .position(|arg| *arg == flag)
                .and_then(|at| args.get(at + 1))
                .map(|value| (*value).to_owned())
        };
        let kind = |prefix: &str| -> String {
            let filter = after("--filter").unwrap_or_default();
            let label = filter.trim_start_matches("label=");
            let matching = labelled
                .iter()
                .filter(|(name, value)| name.starts_with(prefix) && value.as_str() == label);
            matching
                .map(|(name, _)| name.trim_start_matches(prefix).to_owned())
                .collect::<Vec<_>>()
                .join("\n")
        };
        match args {
            ["network", "create", ..] => {
                let name = args.last().copied().unwrap_or_default();
                labelled.insert(
                    format!("network:{name}"),
                    after("--label").unwrap_or_default(),
                );
            }
            ["run", "-d", ..] => {
                let name = after("--name").unwrap_or_default();
                labelled.insert(
                    format!("container:{name}"),
                    after("--label").unwrap_or_default(),
                );
            }
            ["port", ..] => return Err(io::Error::other("no public port")),
            ["ps", ..] => return Ok(kind("container:")),
            ["network", "ls", ..] => return Ok(kind("network:")),
            ["volume", "ls", ..] => return Ok(kind("volume:")),
            ["volume", "rm", "-f", ids @ ..] => {
                for id in ids {
                    labelled.remove(&format!("volume:{id}"));
                }
            }
            ["rm", "-f", ids @ ..] => {
                for id in ids {
                    labelled.remove(&format!("container:{id}"));
                }
            }
            ["network", "rm", ids @ ..] => {
                for id in ids {
                    labelled.remove(&format!("network:{id}"));
                }
            }
            _ => {}
        }
        Ok(String::new())
    }
}
