use crate::AnsibleError;

pub const VAULT_HEADER: &str = "$ANSIBLE_VAULT;";

#[must_use]
pub fn check_vault(files: &[(&str, &str)]) -> Vec<AnsibleError> {
    let mut findings: Vec<AnsibleError> = files
        .iter()
        .filter(|(_, text)| text.starts_with(VAULT_HEADER))
        .map(|(path, _)| AnsibleError::new("ansible_vault_in_ci", path))
        .collect();
    findings.sort();
    findings
}
