struct ActiveGeneration {
    id: String,
    path: PathBuf,
    state: RootHighWater,
}

struct StagedGeneration {
    id: String,
    path: PathBuf,
    previous: Option<String>,
}
