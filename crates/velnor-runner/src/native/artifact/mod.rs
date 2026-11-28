// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

mod blob;
mod download;
mod inputs;
mod js_text;
mod mtime;
mod name_pattern;
mod redirect;
mod retry;
mod search;
mod transfer;
mod twirp;
mod tz;
mod upload;
mod wire;
mod zip;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::FutureExt;
use tokio::sync::mpsc;

use self::inputs::{DOWNLOAD, DOWNLOAD_INPUTS, UPLOAD, UPLOAD_INPUTS};
use crate::exec::{ExecError, StepAdapter, StepContext, StepFuture, StepResult};

#[derive(Debug, Clone, Copy)]
enum Action {
    Upload,
    Download,
}

const UPLOAD_REVISIONS: [&str; 3] = ["cf430e030ddbb5b0abf93d22962f4752f3646cd9", "v7", "v7.0.2"];
const DOWNLOAD_REVISIONS: [&str; 3] = ["9000827ccba6bdab643e8b6fd33ac0654aef8333", "v8", "v8.0.2"];

fn action(reference: &str) -> Option<Action> {
    let (name, revision) = reference.split_once('@')?;
    if name.eq_ignore_ascii_case(UPLOAD) && UPLOAD_REVISIONS.contains(&revision) {
        Some(Action::Upload)
    } else if name.eq_ignore_ascii_case(DOWNLOAD) && DOWNLOAD_REVISIONS.contains(&revision) {
        Some(Action::Download)
    } else {
        None
    }
}

const PATH_CHARACTERS: [(char, &str); 9] = [
    ('"', " Double quote \""),
    (':', " Colon :"),
    ('<', " Less than <"),
    ('>', " Greater than >"),
    ('|', " Vertical bar |"),
    ('*', " Asterisk *"),
    ('?', " Question mark ?"),
    ('\r', " Carriage return \\r"),
    ('\n', " Line feed \\n"),
];

const NAME_ONLY_CHARACTERS: [(char, &str); 2] =
    [('\\', " Backslash \\"), ('/', " Forward slash /")];

const BLANK_LINE: &str = "\n          \n";

fn listed(characters: &[(char, &str)]) -> String {
    characters
        .iter()
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(",")
}

fn validate_path(path: &str) -> Result<(), ExecError> {
    let Some((_, name)) = PATH_CHARACTERS
        .iter()
        .find(|(character, _)| path.contains(*character))
    else {
        return Ok(());
    };
    Err(ExecError::Invalid(format!(
        "The path for one of the files in artifact is not valid: {path}. Contains the following character: {name}{BLANK_LINE}Invalid characters include: {}{BLANK_LINE}The following characters are not allowed in files that are uploaded due to limitations with certain file systems such as NTFS. To maintain file system agnostic behavior, these characters are intentionally not allowed to prevent potential problems with downloads on different file systems.\n          ",
        listed(&PATH_CHARACTERS)
    )))
}

fn invalid_name(name: &str, found: &str) -> ExecError {
    let every: Vec<(char, &str)> = PATH_CHARACTERS
        .iter()
        .chain(&NAME_ONLY_CHARACTERS)
        .copied()
        .collect();
    ExecError::Invalid(format!(
        "The artifact name is not valid: {name}. Contains the following character: {found}{BLANK_LINE}Invalid characters include: {}{BLANK_LINE}These characters are not allowed in the artifact name due to limitations with certain file systems such as NTFS. To maintain file system agnostic behavior, these characters are intentionally not allowed to prevent potential problems with downloads on different file systems.",
        listed(&every)
    ))
}

fn validate_download_name(name: &str) -> Result<(), ExecError> {
    if name == "." || name == ".." {
        return Err(ExecError::Invalid(format!(
            "artifact name '{name}' cannot be extracted into a directory of its own, which downloading several artifacts requires"
        )));
    }
    validate_name(name)
}

fn validate_name(name: &str) -> Result<(), ExecError> {
    if name.is_empty() {
        return Err(ExecError::Invalid(
            "Provided artifact name input during validation is empty".to_owned(),
        ));
    }
    match PATH_CHARACTERS
        .iter()
        .chain(&NAME_ONLY_CHARACTERS)
        .find(|(character, _)| name.contains(*character))
    {
        Some((_, found)) => Err(invalid_name(name, found)),
        None if name.contains('\0') => Err(ExecError::Invalid(format!(
            "The artifact name is not valid: {name}. Velnor refuses a null character in a name, which the toolkit does not check."
        ))),
        None => Ok(()),
    }
}

fn escape(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn client(ctx: &StepContext<'_>) -> Result<twirp::Client, ExecError> {
    let scope = ctx
        .results
        .as_ref()
        .ok_or_else(|| ExecError::Invalid("the job has no Results Service endpoint".to_owned()))?;
    twirp::Client::new(scope).map_err(|error| ExecError::Invalid(format!("{error:#}")))
}

fn failed(error: &impl std::fmt::Display) -> ExecError {
    ExecError::Invalid(format!("{error:#}"))
}

#[derive(Debug)]
pub(super) struct ArtifactAdapter;

struct Blocking {
    stop: Arc<AtomicBool>,
    alive: mpsc::Sender<()>,
}

impl Blocking {
    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
    ) -> Result<T, ExecError> {
        let stop = Arc::clone(&self.stop);
        let alive = self.alive.clone();
        tokio::task::spawn_blocking(move || {
            let _alive = alive;
            work(&stop)
        })
        .await
        .map_err(|error| failed(&error))
    }
}

async fn dispatch(ctx: &mut StepContext<'_>, blocking: &Blocking) -> Result<StepResult, ExecError> {
    let result = match action(&ctx.reference) {
        Some(Action::Upload) => upload::run(ctx, blocking).await,
        Some(Action::Download) => download::run(ctx, blocking).await,
        None => Err(ExecError::Unsupported(format!("uses {}", ctx.reference))),
    };
    match result {
        Err(error @ ExecError::Unsupported(_)) => Err(error),
        Err(error) => {
            ctx.log(&format!("::error::{}", escape(&error.to_string())));
            Ok(StepResult::Failed)
        }
        Ok(step) => Ok(step),
    }
}

impl StepAdapter for ArtifactAdapter {
    fn handles(&self, reference: &str) -> bool {
        action(reference).is_some()
    }

    fn unsupported_input(
        &self,
        reference: &str,
        name: &str,
        value: Option<&str>,
    ) -> Option<String> {
        match action(reference)? {
            Action::Upload => inputs::unsupported(UPLOAD, &UPLOAD_INPUTS, name),
            Action::Download => {
                inputs::unsupported(DOWNLOAD, &DOWNLOAD_INPUTS, name).or_else(|| {
                    value
                        .filter(|_| name.eq_ignore_ascii_case("pattern"))
                        .and_then(name_pattern::unsupported_pattern)
                })
            }
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let stop = Arc::new(AtomicBool::new(false));
            let (alive, mut finished) = mpsc::channel(1);
            let blocking = Blocking {
                stop: Arc::clone(&stop),
                alive,
            };
            let result = match ctx.cancel.clone() {
                Some(cancel) => tokio::select! {
                    result = dispatch(ctx, &blocking) => result,
                    () = cancel.cancelled() => Ok(StepResult::Cancelled),
                },
                None => dispatch(ctx, &blocking).await,
            };
            stop.store(true, Ordering::Relaxed);
            drop(blocking);
            finished.recv().await;
            result
        }
        .boxed()
    }
}
