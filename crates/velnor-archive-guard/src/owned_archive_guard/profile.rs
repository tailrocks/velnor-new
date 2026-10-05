#[derive(Clone, Copy)]
pub(super) enum ArchiveEncoding {
    RawTar,
    Gzip,
}

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub(super) encoding: ArchiveEncoding,
    pub(super) input_limit: u64,
    pub(super) archive_limit: u64,
    pub(super) payload_limit: u64,
    pub(super) member_limit: usize,
    pub(super) allowed_types: u8,
    pub(super) allow_pax: bool,
}

const CARGO_PACKAGE_INPUT_LIMIT: u64 = 128 * 1024 * 1024;
const CARGO_PACKAGE_PAYLOAD_LIMIT: u64 = 512 * 1024 * 1024;
const LARGE_ARCHIVE_LIMIT: u64 = CARGO_PACKAGE_PAYLOAD_LIMIT + 32 * 1024 * 1024;
const CANDIDATE_PAYLOAD_LIMIT: u64 = 1024 * 1024 * 1024;
const CANDIDATE_ARCHIVE_LIMIT: u64 = CANDIDATE_PAYLOAD_LIMIT + 64 * 1024 * 1024;
const MANY_MEMBERS: usize = 30_000;
const CANDIDATE_MEMBERS: usize = 16;

const REGULAR_DIRECTORY: u8 = 0b011;

pub(super) fn profile(mode: &str) -> Result<Profile, String> {
    match mode {
        "candidate" => Ok(candidate_profile()),
        "cargo-package" => Ok(cargo_package_profile()),
        _ => Err(format!("unknown archive guard mode: {mode}")),
    }
}

fn candidate_profile() -> Profile {
    Profile {
        encoding: ArchiveEncoding::RawTar,
        input_limit: CANDIDATE_ARCHIVE_LIMIT,
        archive_limit: CANDIDATE_ARCHIVE_LIMIT,
        payload_limit: CANDIDATE_PAYLOAD_LIMIT,
        member_limit: CANDIDATE_MEMBERS,
        allowed_types: REGULAR_DIRECTORY,
        allow_pax: false,
    }
}

fn cargo_package_profile() -> Profile {
    Profile {
        encoding: ArchiveEncoding::Gzip,
        input_limit: CARGO_PACKAGE_INPUT_LIMIT,
        archive_limit: LARGE_ARCHIVE_LIMIT,
        payload_limit: CARGO_PACKAGE_PAYLOAD_LIMIT,
        member_limit: MANY_MEMBERS,
        allowed_types: REGULAR_DIRECTORY,
        allow_pax: true,
    }
}
