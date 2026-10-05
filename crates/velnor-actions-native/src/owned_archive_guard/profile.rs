#[derive(Clone, Copy)]
pub(super) enum Compression {
    Raw,
    Gzip,
}

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub(super) compression: Compression,
    pub(super) input_limit: u64,
    pub(super) archive_limit: u64,
    pub(super) payload_limit: u64,
    pub(super) member_limit: usize,
    pub(super) allowed_types: u8,
    pub(super) allow_pax: bool,
    pub(super) allow_gnu_long: bool,
}

const SOURCE_INPUT_LIMIT: u64 = 128 * 1024 * 1024;
const SOURCE_PAYLOAD_LIMIT: u64 = 512 * 1024 * 1024;
const LARGE_ARCHIVE_LIMIT: u64 = 512 * 1024 * 1024 + 32 * 1024 * 1024;
const CANDIDATE_PAYLOAD_LIMIT: u64 = 1024 * 1024 * 1024;
const CANDIDATE_ARCHIVE_LIMIT: u64 = CANDIDATE_PAYLOAD_LIMIT + 64 * 1024 * 1024;
const SEMVER_INPUT_LIMIT: u64 = 32 * 1024 * 1024;
const SEMVER_PAYLOAD_LIMIT: u64 = 64 * 1024 * 1024;
const SEMVER_ARCHIVE_LIMIT: u64 = SEMVER_PAYLOAD_LIMIT + 8 * 1024 * 1024;
const MANY_MEMBERS: usize = 30_000;
const CANDIDATE_MEMBERS: usize = 16;
const SEMVER_MEMBERS: usize = 5_000;

const REGULAR_DIRECTORY: u8 = 0b011;
const REGULAR_DIRECTORY_SYMLINK: u8 = 0b111;

pub(super) fn profile(mode: &str, revision: Option<&str>) -> Result<Profile, String> {
    let policy = match mode {
        "source" => source_profile(),
        "candidate" => candidate_profile(),
        "bootstrap" => bootstrap_profile(),
        "source-publication" => source_publication_profile(),
        "semver-capsule" => semver_profile(),
        "cargo-package" => cargo_package_profile(),
        _ => return Err(format!("unknown archive guard mode: {mode}")),
    };
    validate_revision(mode, revision)?;
    Ok(policy)
}

fn validate_revision(mode: &str, revision: Option<&str>) -> Result<(), String> {
    let needs_revision = mode == "semver-capsule";
    if needs_revision != revision.is_some() {
        return Err("archive guard revision argument mismatch".to_owned());
    }
    if let Some(value) = revision {
        let valid = value.len() == 40
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return Err("archive guard revision must be 40 lowercase hex characters".to_owned());
        }
    }
    Ok(())
}

fn source_profile() -> Profile {
    Profile {
        compression: Compression::Raw,
        input_limit: SOURCE_INPUT_LIMIT,
        archive_limit: LARGE_ARCHIVE_LIMIT,
        payload_limit: SOURCE_PAYLOAD_LIMIT,
        member_limit: MANY_MEMBERS,
        allowed_types: REGULAR_DIRECTORY_SYMLINK,
        allow_pax: true,
        allow_gnu_long: false,
    }
}

fn candidate_profile() -> Profile {
    Profile {
        compression: Compression::Gzip,
        input_limit: SOURCE_PAYLOAD_LIMIT,
        archive_limit: CANDIDATE_ARCHIVE_LIMIT,
        payload_limit: CANDIDATE_PAYLOAD_LIMIT,
        member_limit: CANDIDATE_MEMBERS,
        allowed_types: REGULAR_DIRECTORY,
        allow_pax: false,
        allow_gnu_long: false,
    }
}

fn bootstrap_profile() -> Profile {
    Profile {
        compression: Compression::Gzip,
        input_limit: SOURCE_INPUT_LIMIT,
        archive_limit: LARGE_ARCHIVE_LIMIT,
        payload_limit: SOURCE_PAYLOAD_LIMIT,
        member_limit: MANY_MEMBERS,
        allowed_types: REGULAR_DIRECTORY,
        allow_pax: true,
        allow_gnu_long: true,
    }
}

fn source_publication_profile() -> Profile {
    Profile {
        compression: Compression::Raw,
        input_limit: SOURCE_PAYLOAD_LIMIT,
        archive_limit: LARGE_ARCHIVE_LIMIT,
        payload_limit: SOURCE_PAYLOAD_LIMIT,
        member_limit: MANY_MEMBERS,
        allowed_types: REGULAR_DIRECTORY_SYMLINK,
        allow_pax: true,
        allow_gnu_long: false,
    }
}

fn semver_profile() -> Profile {
    Profile {
        compression: Compression::Raw,
        input_limit: SEMVER_INPUT_LIMIT,
        archive_limit: SEMVER_ARCHIVE_LIMIT,
        payload_limit: SEMVER_PAYLOAD_LIMIT,
        member_limit: SEMVER_MEMBERS,
        allowed_types: REGULAR_DIRECTORY,
        allow_pax: true,
        allow_gnu_long: false,
    }
}

fn cargo_package_profile() -> Profile {
    Profile {
        compression: Compression::Gzip,
        input_limit: SOURCE_INPUT_LIMIT,
        archive_limit: LARGE_ARCHIVE_LIMIT,
        payload_limit: SOURCE_PAYLOAD_LIMIT,
        member_limit: MANY_MEMBERS,
        allowed_types: REGULAR_DIRECTORY,
        allow_pax: true,
        allow_gnu_long: false,
    }
}
