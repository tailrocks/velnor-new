pub(crate) const NON_UTF8_PATH: &str = "non_utf8_path";

pub(crate) fn split_nul_paths<Paths>(stdout: &[u8]) -> Result<Paths, String>
where
    Paths: FromIterator<String>,
{
    stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|chunk| String::from_utf8(chunk.to_vec()).map_err(|_| NON_UTF8_PATH.to_owned()))
        .collect()
}

pub(crate) fn split_nul_paths_skipping(stdout: &[u8]) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut skipped = false;
    for chunk in stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        match String::from_utf8(chunk.to_vec()) {
            Ok(path) => paths.push(path),
            Err(_) => skipped = true,
        }
    }
    (paths, skipped)
}
