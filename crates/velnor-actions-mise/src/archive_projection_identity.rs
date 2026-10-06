//! Framed identity only; production admission belongs to an exact compiled registry.

use velnor_actions_contract::compiled_source_sha256;

/// Typed roles prevent swapping restore, save, template and runtime identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CapabilityTuple {
    pub(super) action: ImmutableAction,
    pub(super) restore: ActionEntrypoint,
    pub(super) save: ActionEntrypoint,
    pub(super) adapter_source_closure_sha256: String,
    pub(super) inventory_template_sha256: String,
    pub(super) runtime: QualifiedRuntimeClosure,
    pub(super) transform_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ImmutableAction {
    pub(super) repository: String,
    pub(super) commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActionEntrypoint {
    pub(super) path: String,
    pub(super) bundle_sha256: String,
}

/// Closure digest includes executable/import bytes and runtime qualification.
/// Profile, tar, Node and codec ABI are separate fields: none can be omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QualifiedRuntimeClosure {
    pub(super) closure_sha256: String,
    pub(super) host_profile: String,
    pub(super) tar_sha256: String,
    pub(super) node_sha256: String,
    pub(super) codec_abi: String,
}

impl CapabilityTuple {
    pub(super) fn sha256(&self) -> String {
        let fields = [
            self.action.repository.as_bytes(),
            self.action.commit.as_bytes(),
            self.restore.path.as_bytes(),
            self.restore.bundle_sha256.as_bytes(),
            self.save.path.as_bytes(),
            self.save.bundle_sha256.as_bytes(),
            self.adapter_source_closure_sha256.as_bytes(),
            self.inventory_template_sha256.as_bytes(),
            self.runtime.closure_sha256.as_bytes(),
            self.runtime.host_profile.as_bytes(),
            self.runtime.tar_sha256.as_bytes(),
            self.runtime.node_sha256.as_bytes(),
            self.runtime.codec_abi.as_bytes(),
            self.transform_revision.as_bytes(),
        ];
        framed_sha256("capability-v1", &fields)
    }

    #[cfg(test)]
    pub(super) fn validate(&self) -> Result<(), &'static str> {
        let hashes = [
            &self.restore.bundle_sha256,
            &self.save.bundle_sha256,
            &self.adapter_source_closure_sha256,
            &self.inventory_template_sha256,
            &self.runtime.closure_sha256,
            &self.runtime.tar_sha256,
            &self.runtime.node_sha256,
        ];
        if !hash(&self.action.commit, 40) || hashes.iter().any(|value| !hash(value, 64)) {
            return Err("immutable_digest");
        }
        let repository: Vec<_> = self.action.repository.split('/').collect();
        if repository.len() != 2 || repository.iter().any(|part| !atom(part)) {
            return Err("action_repository");
        }
        for path in [&self.restore.path, &self.save.path] {
            if path.split('/').any(|part| !atom(part)) {
                return Err("entrypoint");
            }
        }
        for value in [
            &self.runtime.host_profile,
            &self.runtime.codec_abi,
            &self.transform_revision,
        ] {
            if !atom(value) {
                return Err("runtime_or_revision");
            }
        }
        if self.restore.path == self.save.path {
            return Err("entrypoint_roles");
        }
        Ok(())
    }
}

/// Every field, including purpose and field count, uses an unsigned BE u64 frame.
pub(super) fn framed_sha256(purpose: &str, fields: &[&[u8]]) -> String {
    let mut bytes = Vec::new();
    frame(&mut bytes, b"velnor-source-archive-projection-v1");
    frame(&mut bytes, purpose.as_bytes());
    bytes.extend_from_slice(&(fields.len() as u64).to_be_bytes());
    for field in fields {
        frame(&mut bytes, field);
    }
    compiled_source_sha256(&bytes)
}

fn frame(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

#[cfg(test)]
fn hash(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
fn atom(value: &str) -> bool {
    !matches!(value, "" | "." | "..")
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
