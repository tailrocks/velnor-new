use super::*;

#[test]
fn repository_policy_defaults_to_no_ref_rules_and_rejects_explicit_restrictions()
-> Result<(), String> {
    let dir = TempDir::new("repository-policy")?;
    let mut repository = request(dir.path());
    repository.source_policy_mode = "repository";
    repository.branches.clear();
    let id = velnor_runner_host::ConnectionId::parse(repository.connection_id)
        .map_err(|error| error.to_string())?;
    let credential = velnor_runner_host::KeychainReference::parse("keychain:test/repo")
        .map_err(|error| error.to_string())?;
    let connection =
        connection_from(&repository, id, credential).map_err(|error| error.to_string())?;
    let config = sample_config(&repository, &connection);
    if !config.contains("mode = \"repository\"") || !config.contains("branches = []") {
        return Err("repository policy gained an implicit ref restriction".to_owned());
    }

    let mut branch = request(dir.path());
    branch.source_policy_mode = "repository";
    if connection_from(
        &branch,
        velnor_runner_host::ConnectionId::parse(branch.connection_id)
            .map_err(|error| error.to_string())?,
        velnor_runner_host::KeychainReference::parse("keychain:test/repo")
            .map_err(|error| error.to_string())?,
    )
    .is_ok()
    {
        return Err("repository mode silently discarded an explicit branch".to_owned());
    }

    let mut tag = request(dir.path());
    tag.source_policy_mode = "repository";
    tag.branches.clear();
    tag.tag_prefixes.push("v".to_owned());
    if connection_from(
        &tag,
        velnor_runner_host::ConnectionId::parse(tag.connection_id)
            .map_err(|error| error.to_string())?,
        velnor_runner_host::KeychainReference::parse("keychain:test/repo")
            .map_err(|error| error.to_string())?,
    )
    .is_ok()
    {
        return Err("repository mode silently discarded an explicit tag prefix".to_owned());
    }

    let mut pull_request = request(dir.path());
    pull_request.source_policy_mode = "repository";
    pull_request.branches.clear();
    pull_request
        .pull_request_rules
        .push("*:123:main:456:patch".to_owned());
    if connection_from(
        &pull_request,
        velnor_runner_host::ConnectionId::parse(pull_request.connection_id)
            .map_err(|error| error.to_string())?,
        velnor_runner_host::KeychainReference::parse("keychain:test/repo")
            .map_err(|error| error.to_string())?,
    )
    .is_ok()
    {
        return Err("repository mode silently discarded explicit PR rules".to_owned());
    }
    Ok(())
}
