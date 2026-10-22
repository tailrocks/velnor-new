use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::{
    BoundJobFactsSnapshot, Cache, Document, Engine, InstanceBinding, JobFactsMount, Profile,
    Reservation, Runner, Sample, Scope,
};

impl Document {
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn encode(
        mut self,
        physical_binding_digest: String,
        runner_directory_inode: u64,
    ) -> Result<BoundJobFactsSnapshot, HostError> {
        self.instance_binding_digest =
            hash(b"velnor-instance-binding-v1\0", &self.instance_binding)?;
        self.projection_digest = projection_digest(&self)?;
        let bytes = serde_json::to_vec(&self).map_err(|_| HostError::Identity)?;
        if bytes.is_empty() || bytes.len() > 65_536 || runner_directory_inode == 0 {
            return Err(HostError::Identity);
        }
        Ok(BoundJobFactsSnapshot {
            bytes,
            projection_digest: self.projection_digest,
            physical_binding_digest,
            runner_directory_inode,
        })
    }
}

fn projection_digest(document: &Document) -> Result<String, HostError> {
    #[derive(Serialize)]
    struct Preimage<'a> {
        schema: &'a str,
        version: u32,
        scope: &'a Scope,
        reservation: &'a Reservation,
        runner: &'a Runner,
        engine: &'a Engine,
        profile: &'a Profile,
        sample: &'a Sample,
        instance_binding: &'a InstanceBinding,
        instance_binding_digest: &'a str,
        guest_image_sha256: &'a str,
        cache: &'a Cache,
        job_facts_mount: &'a JobFactsMount,
    }
    let bytes = serde_json::to_vec(&Preimage {
        schema: &document.schema,
        version: document.version,
        scope: &document.scope,
        reservation: &document.reservation,
        runner: &document.runner,
        engine: &document.engine,
        profile: &document.profile,
        sample: &document.sample,
        instance_binding: &document.instance_binding,
        instance_binding_digest: &document.instance_binding_digest,
        guest_image_sha256: &document.guest_image_sha256,
        cache: &document.cache,
        job_facts_mount: &document.job_facts_mount,
    })
    .map_err(|_| HostError::Identity)?;
    Ok(hash_bytes(
        b"velnor-storage-cache-admission-projection-v1\0",
        &bytes,
    ))
}

fn hash_bytes(domain: &[u8], bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(bytes);
    lower_hex(&digest.finalize())
}

fn hash<T: Serialize>(domain: &[u8], value: &T) -> Result<String, HostError> {
    let bytes = serde_json::to_vec(value).map_err(|_| HostError::Identity)?;
    Ok(hash_bytes(domain, &bytes))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
