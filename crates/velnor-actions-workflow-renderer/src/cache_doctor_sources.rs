use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::CacheFamily;

use super::{Family, RenderError, doctor};

/// I3: no other family archives or nests a sources path.
pub(super) fn check_sources_isolation(
    owners: &BTreeMap<String, Family>,
) -> Result<(), RenderError> {
    let sources = CacheFamily::Sources.spec().resolved_paths(None);
    for (path, family) in owners {
        if *family == Family::Runtime(CacheFamily::Sources) {
            continue;
        }
        let overlaps = sources.iter().any(|source| {
            path == source
                || path.starts_with(&format!("{source}/"))
                || source.starts_with(&format!("{path}/"))
        });
        if overlaps {
            return Err(doctor(
                "workflow",
                &format!("i3_sources_path_shared:{path}"),
            ));
        }
    }
    Ok(())
}
