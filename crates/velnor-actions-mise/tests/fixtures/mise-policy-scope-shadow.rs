use std::fs::File as Handle;

pub mod inert {
    pub type Handle = std::path::Path;
}

pub fn destination(path: &std::path::Path) -> std::io::Result<Handle> {
    Handle::create(path)
}
