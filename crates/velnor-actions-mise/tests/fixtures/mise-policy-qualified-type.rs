use std::fs::File as Handle;

pub fn destination(path: &std::path::Path) -> std::io::Result<Handle> {
    <Handle>::create(path)
}

pub fn options_factory() {
    let _options = <Handle>::options;
}
