// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
