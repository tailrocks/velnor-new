use std::path::Path;

use std as standard;

type PathAlias = Path;

pub fn remove_through_function_alias(path: &PathAlias) -> std::io::Result<()> {
    fn remove(path: &PathAlias) -> std::io::Result<()> {
        standard::fs::remove_file(path)
    }
    let remove: fn(&PathAlias) -> std::io::Result<()> = remove;
    remove(path)
}
