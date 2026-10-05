pub(super) fn declaration(name: &str, compact: &str) -> Option<(String, String)> {
    let explicit = format!("#[cfg(test)]#[path=\"{name}\"]mod");
    let ordinary = (name == "tests.rs").then_some("#[cfg(test)]mod");
    let (start, prefix_length) = if let Some(start) = compact.find(&explicit) {
        (start, explicit.len())
    } else {
        let prefix = ordinary?;
        (compact.find(prefix)?, prefix.len())
    };
    let module_start = start.checked_add(prefix_length)?;
    let module_end = compact[module_start..]
        .find(';')?
        .checked_add(module_start)?;
    let module = &compact[module_start..module_end];
    if module.is_empty()
        || !module
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    Some((compact[start..=module_end].to_owned(), module.to_owned()))
}
