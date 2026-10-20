use super::*;

#[test]
fn invalid_authority_policy_is_rejected_before_token_storage() -> Result<(), String> {
    let dir = TempDir::new("invalid-authority-policy")?;
    let policy_path = dir.path().join("policy.toml");
    std::fs::write(&policy_path, "[[job_authority_policies]]\nunknown = true\n")
        .map_err(|error| error.to_string())?;
    let mut request = request(dir.path());
    request.job_authority_policy = Some(&policy_path);
    let stored = RefCell::new(false);
    let mut input = Cursor::new(b"token\n");
    let result = connect_with_store(&mut input, TEST_SERVICE, &request, |_, _, _| {
        *stored.borrow_mut() = true;
        Ok(())
    });
    if result.is_ok() || *stored.borrow() || dir.path().join("host.toml").exists() {
        return Err("invalid policy was stored or written".to_owned());
    }
    Ok(())
}

#[test]
fn incomplete_authority_inventory_is_rejected_before_token_storage() -> Result<(), String> {
    let dir = TempDir::new("incomplete-authority-inventory")?;
    let inventory_path = dir.path().join("inventory.json");
    std::fs::write(&inventory_path, b"{}").map_err(|error| error.to_string())?;
    let mut request = request(dir.path());
    request.job_authority_inventory = Some(&inventory_path);
    let stored = RefCell::new(false);
    let mut input = Cursor::new(b"token\n");
    let result = connect_with_store(&mut input, TEST_SERVICE, &request, |_, _, _| {
        *stored.borrow_mut() = true;
        Ok(())
    });
    if result.is_ok() || *stored.borrow() || dir.path().join("host.toml").exists() {
        return Err("incomplete inventory identity was stored or written".to_owned());
    }
    Ok(())
}

#[test]
fn non_regular_authority_input_is_rejected_before_token_storage() -> Result<(), String> {
    let dir = TempDir::new("non-regular-authority-input")?;
    let source_sha = "a".repeat(40);
    let artifact_sha256 = "b".repeat(64);
    let mut request = request(dir.path());
    request.job_authority_inventory = Some(dir.path());
    request.job_authority_generator_source_sha = Some(&source_sha);
    request.job_authority_generator_artifact_sha256 = Some(&artifact_sha256);
    request.job_authority_generation = Some(1);
    let stored = RefCell::new(false);
    let mut input = Cursor::new(b"token\n");
    let result = connect_with_store(&mut input, TEST_SERVICE, &request, |_, _, _| {
        *stored.borrow_mut() = true;
        Ok(())
    });
    if result.is_ok() || *stored.borrow() || dir.path().join("host.toml").exists() {
        return Err("non-regular authority input was stored or written".to_owned());
    }
    Ok(())
}

#[test]
fn inventory_connect_stores_token_and_persists_valid_bound_policy() -> Result<(), String> {
    let dir = TempDir::new("inventory-connect")?;
    let inventory_path = dir.path().join("inventory.json");
    std::fs::write(&inventory_path, valid_inventory_json()).map_err(|error| error.to_string())?;
    let source_sha = "a".repeat(40);
    let artifact_sha256 = "b".repeat(64);
    let mut request = request(dir.path());
    request.job_authority_inventory = Some(&inventory_path);
    request.job_authority_generator_source_sha = Some(&source_sha);
    request.job_authority_generator_artifact_sha256 = Some(&artifact_sha256);
    request.job_authority_generation = Some(1);
    let token = b"inventory-token\n";
    let stored = RefCell::new(None);
    let mut input = Cursor::new(&token[..]);
    connect_with_store(
        &mut input,
        TEST_SERVICE,
        &request,
        |service, account, secret| {
            *stored.borrow_mut() = Some((service.to_owned(), account.to_owned(), secret.to_vec()));
            Ok(())
        },
    )
    .map_err(|error| error.to_string())?;
    let text =
        std::fs::read_to_string(dir.path().join("host.toml")).map_err(|error| error.to_string())?;
    let reopened =
        velnor_runner_host::HostConfig::parse(&text).map_err(|error| error.to_string())?;
    if !text.contains("[[job_authority_policies]]")
        || reopened.to_toml().map_err(|error| error.to_string())? != text
    {
        return Err("persisted policy did not validate on reopen".to_owned());
    }
    let stored = stored
        .into_inner()
        .ok_or("credential store was not called")?;
    if stored.0 != TEST_SERVICE || stored.1 != "example-repo" || stored.2.as_slice() != token {
        return Err("credential store arguments did not match this connection".to_owned());
    }

    let retried = RefCell::new(false);
    let mut retry_token = Cursor::new(b"retry-token\n");
    if connect_with_store(&mut retry_token, TEST_SERVICE, &request, |_, _, _| {
        *retried.borrow_mut() = true;
        Ok(())
    })
    .is_ok()
        || *retried.borrow()
    {
        return Err("repeat policy import reached credential storage".to_owned());
    }
    Ok(())
}

fn valid_inventory_json() -> &'static str {
    r#"{"schema_version":1,"generator_source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","inventory_sha256":"c47ac0910d6a63868c612c6828e02c83429d95b1b5a58036f5d1e15c214a21c2","workflows":[{"path":".github/workflows/ci.yml","content_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","calls":[],"jobs":[{"root_path":".github/workflows/ci.yml","workflow_path":".github/workflows/ci.yml","job_key":"required","role":"verification","contexts":[{"event":"push","branch_filters":["main"],"tag_filters":[],"pull_request_base_branches":[]}],"permissions_declared":true,"permissions":{"contents":"read"},"secrets":[],"environments":[],"condition_sha256":null,"concurrency":null,"needs":[]}]}]}"#
}
