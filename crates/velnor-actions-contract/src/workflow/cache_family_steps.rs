use super::cache_family::CacheFamily;

pub const FAMILY_KEYS_ID: &str = "family-keys";

pub const FAMILY_SAVE_OUTPUT: &str = "save";

pub const FAMILY_ADMITTED_OUTPUT: &str = "admitted";

pub const FAMILY_WRITER_ENV: &str = "VELNOR_FAMILY_WRITER";

pub const FAMILY_WRITER_EXPR: &str = "${{ matrix.leg }}";

pub const STACK_FAMILIES: [CacheFamily; 5] = [
    CacheFamily::BunInstall,
    CacheFamily::PlaywrightBrowsers,
    CacheFamily::DbtPackages,
    CacheFamily::PythonVenv,
    CacheFamily::AnsibleCollections,
];

#[must_use]
pub fn is_stack_family(family: CacheFamily) -> bool {
    STACK_FAMILIES.contains(&family)
}

#[must_use]
pub fn family_output_stem(family: CacheFamily) -> String {
    family.spec().layer.replace('-', "_")
}

#[must_use]
pub fn family_for_stem(stem: &str) -> Option<CacheFamily> {
    STACK_FAMILIES
        .into_iter()
        .find(|family| family_output_stem(*family) == stem)
}

#[must_use]
pub fn family_key_expr(family: CacheFamily) -> String {
    format!(
        "${{{{steps.{FAMILY_KEYS_ID}.outputs.{}_key}}}}",
        family_output_stem(family)
    )
}

#[must_use]
pub fn family_restore_keys_expr(family: CacheFamily) -> String {
    format!(
        "${{{{steps.{FAMILY_KEYS_ID}.outputs.{}_restore_keys}}}}",
        family_output_stem(family)
    )
}

#[must_use]
pub fn family_size_step_id(family: CacheFamily) -> String {
    format!("family-size-{}", family.spec().layer)
}

#[must_use]
pub fn family_save_condition(family: CacheFamily) -> String {
    format!(
        "success() && steps.{FAMILY_KEYS_ID}.outputs.{FAMILY_SAVE_OUTPUT} == 'true' && steps.{}.outputs.{FAMILY_ADMITTED_OUTPUT} == 'true'",
        family_size_step_id(family)
    )
}

#[must_use]
pub fn root_locator(root: &str) -> Option<String> {
    let path = root.trim_matches('/');
    if path.is_empty() || path == "." {
        return Some("root".to_owned());
    }
    let safe = root.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    (!safe).then(|| path.replace('/', "-"))
}
