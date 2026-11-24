// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOpenssl {
    None,
    Sysroot,
    Vendored,
}

impl ImageOpenssl {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sysroot => "sysroot",
            Self::Vendored => "vendored",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePrBuild {
    Inputs,
    Always,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageHost {
    DockerHub,
    Ghcr,
}

impl ImageHost {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DockerHub => "docker.io",
            Self::Ghcr => "ghcr.io",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageVisibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageCredential {
    pub context: String,
    pub name: String,
}

impl ImageCredential {
    #[must_use]
    pub fn expression(&self) -> String {
        format!("${{{{ {}.{} }}}}", self.context, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRegistry {
    pub host: ImageHost,
    pub repository: String,
    pub username: Option<ImageCredential>,
    pub token: Option<ImageCredential>,
    pub visibility: ImageVisibility,
}

impl ImageRegistry {
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{}/{}", self.host.name(), self.repository)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageConfig {
    pub name: String,
    pub bins: Vec<String>,
    pub package: Option<String>,
    pub root: Option<String>,
    pub features: Vec<String>,
    pub default_features: bool,
    pub profile: String,
    pub dockerfile: String,
    pub context: String,
    pub stage: String,
    pub inputs: Vec<String>,
    pub glibc: String,
    pub openssl: ImageOpenssl,
    pub pr_build: ImagePrBuild,
    pub attestations: bool,
    pub timeout_minutes: u16,
    pub registries: Vec<ImageRegistry>,
}

impl ImageConfig {
    #[must_use]
    pub fn profile_directory(&self) -> &str {
        if self.profile == "dev" {
            "debug"
        } else {
            &self.profile
        }
    }

    #[must_use]
    pub fn tag_glob(&self) -> String {
        format!("{}-v[0-9]*.[0-9]*.[0-9]*", self.name)
    }
}
