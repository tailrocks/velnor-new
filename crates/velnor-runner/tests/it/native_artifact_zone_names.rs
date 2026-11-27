// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use anyhow::Result;
use jiff::Timestamp;

use super::native_artifact_zones::{Clock, PACIFIC, Setup, System, assert_near, expiry_for};

const NAMES_THE_IMAGE_LACKS: [(&str, &str); 5] = [
    ("US/Pacific", PACIFIC),
    ("PST8PDT", PACIFIC),
    ("EST5EDT", "America/New_York"),
    ("GB", "Europe/London"),
    ("NZ", "Pacific/Auckland"),
];

const NAMES_ICU_DROPS_FOR_A_DIFFERING_STANDARD_OFFSET: [(&str, &str); 4] = [
    ("CET", "Europe/Brussels"),
    ("EET", "Europe/Athens"),
    ("MET", "Europe/Brussels"),
    ("Cuba", "America/Havana"),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_the_image_lacks_come_from_the_bundled_database_not_the_host() -> Result<()> {
    let setup = Setup {
        zoneinfo: true,
        system: System::Flat,
        tz: None,
    };
    for (row, zone) in NAMES_THE_IMAGE_LACKS {
        let clock = Clock::pick(&[zone])?;
        let sent = expiry_for(&clock, &[], Some(row), setup).await?;
        assert_near(sent, clock.calendar(zone)?, row, &clock);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_icu_drops_for_a_differing_standard_offset_add_whole_days_though_the_bundle_lists_them()
-> Result<()> {
    for (row, zone) in NAMES_ICU_DROPS_FOR_A_DIFFERING_STANDARD_OFFSET {
        let clock = Clock::pick(&[zone])?;
        let sent = expiry_for(&clock, &[], Some(row), Setup::PACIFIC).await?;
        assert_near(sent, clock.flat, row, &clock);
        let named = expiry_for(&clock, &[], Some(zone), Setup::PACIFIC).await?;
        assert_near(named, clock.calendar(zone)?, zone, &clock);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_tz_that_is_not_utf8_is_read_like_any_other_text() -> Result<()> {
    let clock = Clock::pick(&[PACIFIC])?;
    let cases: [(&[u8], Timestamp); 4] = [
        (b"\xff", clock.flat),
        (b"Australia/Sydney\xff", clock.flat),
        (b"\xffAustralia/Sydney", clock.flat),
        (b"ABC1\xff", clock.calendar(PACIFIC)?),
    ];
    for (bytes, expected) in cases {
        let setup = Setup {
            tz: Some(OsStr::from_bytes(bytes)),
            ..Setup::PACIFIC
        };
        let sent = expiry_for(&clock, &[], None, setup).await?;
        assert_near(sent, expected, &format!("{bytes:?}"), &clock);
    }
    Ok(())
}
