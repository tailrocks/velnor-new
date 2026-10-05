//! Renderer-owned workflow step output IDs.

use crate::yaml::Yaml;

/// Append the fixed output ID for one renderer-owned step name.
pub(crate) fn push_step_id(entries: &mut Vec<(String, Yaml)>, name: &str) {
    if let Some(id) = step_id(name) {
        entries.push(("id".to_owned(), Yaml::str(id.to_owned())));
    } else {
        crate::mbx_bundle::push_step_id(entries, name);
    }
}

fn step_id(name: &str) -> Option<&'static str> {
    match name {
        crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME => {
            Some(crate::cache_p08::TOOLS_CACHE_IDENTITY_STEP_ID)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::push_step_id;
    use crate::yaml::Yaml;

    #[test]
    fn runtime_cache_identity_has_one_renderer_owned_output_id() {
        let mut entries = Vec::new();
        push_step_id(&mut entries, crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME);
        assert_eq!(
            entries,
            vec![(
                "id".to_owned(),
                Yaml::str(crate::cache_p08::TOOLS_CACHE_IDENTITY_STEP_ID.to_owned())
            )]
        );
    }
}
