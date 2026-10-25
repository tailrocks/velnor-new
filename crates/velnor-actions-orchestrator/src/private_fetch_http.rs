use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use velnor_actions_mise::command::IsolatedCommand;

use crate::OrchestratorError;
use crate::config::private_fetch_session::{
    CAPTURE_BYTES, CURL_TIMEOUT_SECS, Token, create_private_dir, curl_config, scrub_dir,
    write_private,
};
use crate::internal::internal;

pub(crate) fn http_status(
    runner_temp: &Path,
    token: &Token,
    method: &str,
    route: &str,
) -> Result<u16, OrchestratorError> {
    let dir = runner_temp.join("velnor").join("private-revoke");
    scrub_dir(&dir)?;
    create_private_dir(&dir)?;
    let result = (|| {
        let file = dir.join("curl.conf");
        let text = curl_config(
            token,
            method,
            route,
            "application/vnd.github+json",
            Path::new("/dev/null"),
            true,
        )?
        .replace("fail-with-body\n", "");
        write_private(&file, &text)?;
        let command = IsolatedCommand::private_fetch(
            "curl",
            vec![OsString::from("--config"), file.as_os_str().to_owned()],
            dir.clone(),
            &dir.join("unused-gitconfig"),
            &[],
            None,
        )
        .map_err(|err| internal(&format!("private_curl_command:{err}")))?;
        let output = command
            .run_bounded(CAPTURE_BYTES, Duration::from_secs(CURL_TIMEOUT_SECS))
            .map_err(|err| internal(&format!("private_curl_spawn:{err}")))?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u16>()
            .map_err(|_| internal("private_http_status_unreadable"))
    })();
    let cleanup = scrub_dir(&dir);
    let status = result?;
    cleanup?;
    Ok(status)
}
