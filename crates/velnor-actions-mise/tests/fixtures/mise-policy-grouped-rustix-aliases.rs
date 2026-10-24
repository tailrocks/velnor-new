use {rustix::fs as stage_ops, std::fs::File};
pub fn destination(path: &std::path::Path) -> std::io::Result<File> {
    let fd = stage_ops::open(
        path,
        stage_ops::OFlags::WRONLY | stage_ops::OFlags::CREATE | stage_ops::OFlags::TRUNC,
        stage_ops::Mode::RUSR | stage_ops::Mode::WUSR,
    )?;
    Ok(File::from(fd))
}
