use velnor_runner_github::{
    AdminConnectionCall, RegistrationScope as GithubRegistrationScope, RegistrationTokenCall,
    admin_connection, registration_token,
};
use velnor_runner_host_config::RegistrationScope;

use super::{EnsureError, Recording, map_host, map_session};
use crate::https::HttpsTransport;

const GITHUB_API: &str = "https://api.github.com";

pub(super) fn open_admin(
    pat: &str,
    scope: &RegistrationScope,
) -> Result<(Recording, velnor_runner_github::AdminConnection), EnsureError> {
    let (github_scope, config_url) = match scope {
        RegistrationScope::Repository { owner, repository } => (
            GithubRegistrationScope::Repository {
                owner,
                repo: repository,
            },
            format!("https://github.com/{owner}/{repository}"),
        ),
        RegistrationScope::Organization { organization } => (
            GithubRegistrationScope::Organization { org: organization },
            format!("https://github.com/{organization}"),
        ),
    };
    let mut transport = Recording::new(HttpsTransport::new(GITHUB_API).map_err(map_host)?);
    let registration = registration_token(
        &mut transport,
        &RegistrationTokenCall {
            scope: github_scope,
            pat,
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    let admin = admin_connection(
        &mut transport,
        &AdminConnectionCall {
            config_url: &config_url,
            registration_token: registration.expose(),
        },
    )
    .map_err(|err| map_session(err, &transport))?;
    transport
        .inner
        .set_base(admin.expose_url())
        .map_err(map_host)?;
    Ok((transport, admin))
}
