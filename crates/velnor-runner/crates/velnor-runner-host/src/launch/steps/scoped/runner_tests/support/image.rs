use serde_json::json;

use crate::launch::{VerifiedImageIdentity, VerifiedWorkerImages};

pub(in crate::launch::steps::scoped::runner::tests) fn image_inspect(
    images: &VerifiedWorkerImages,
    name: &str,
    identity: &VerifiedImageIdentity,
) -> String {
    let reference = format!(
        "{name}:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    let index = identity
        .image_index_digest()
        .expect("worker image fixture has an index");
    let tag = reference.split_once(':').expect("image has a tag").1;
    json!({
        "Id": identity.runtime_id(),
        "Os": "linux",
        "Architecture": "amd64",
        "RepoTags": [reference],
        "RepoDigests": [format!("docker.io/library/{name}@{index}")],
        "Descriptor": {
            "digest": index,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "size": identity.image_index_size().expect("worker index has size"),
            "annotations": {
                "io.containerd.image.name": format!("docker.io/library/{reference}"),
                "org.opencontainers.image.ref.name": tag
            }
        }
    })
    .to_string()
}
