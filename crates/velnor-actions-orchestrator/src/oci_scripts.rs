//! Fixed argv for generated OCI scripts; no inline source expansion.

fn command(mode: &str, repository: &str, ci: &str, branch: &str) -> Vec<String> {
    [mode, repository, ci, branch].map(str::to_owned).to_vec()
}

pub(super) fn verify_script(repository: &str, ci: &str, branch: &str) -> Vec<String> {
    command("verify", repository, ci, branch)
}

pub(super) fn source_script(repository: &str, branch: &str) -> Vec<String> {
    command("source", repository, "-", branch)
}

pub(super) fn admission_script() -> Vec<String> {
    command("admission", "-", "-", "-")
}

pub(super) fn record_script() -> Vec<String> {
    command("record", "-", "-", "-")
}

pub(super) fn artifact_script() -> Vec<String> {
    command("artifact", "-", "-", "-")
}

pub(super) fn publish_admission_script(repository: &str, ci: &str, branch: &str) -> Vec<String> {
    command("publish-admission", repository, ci, branch)
}

pub(super) fn index_receipt_script() -> Vec<String> {
    command("index-receipt", "-", "-", "-")
}

pub(super) fn assembly_script(repository: &str, branch: &str) -> Vec<String> {
    command("assembly", repository, "-", branch)
}

pub(super) fn platform_publish_script(repository: &str, ci: &str, branch: &str) -> Vec<String> {
    command("platform-publish", repository, ci, branch)
}
