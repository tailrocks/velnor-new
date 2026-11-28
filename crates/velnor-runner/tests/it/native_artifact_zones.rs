// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use jiff::tz::TimeZoneDatabase;
use jiff::{Span, Timestamp};

use super::artifact_support::{Env, run_job_env, write_file};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;

pub(super) const PACIFIC: &str = "America/Los_Angeles";
pub(super) const SYDNEY: &str = "Australia/Sydney";

const NAMED: [&str; 7] = [
    "Australia/Sydney",
    ":Australia/Sydney",
    "posix/Australia/Sydney",
    "right/Australia/Sydney",
    ":posix/Australia/Sydney",
    "Australia/ACT",
    "Australia/NSW",
];

const UTC_LIKE: [&str; 36] = [
    "",
    ":",
    "UTC",
    "Z",
    "Bogus/Zone",
    "australia/sydney",
    "AUSTRALIA/SYDNEY",
    " Australia/Sydney",
    "Australia/Sydney ",
    "Posix/Australia/Sydney",
    "::Australia/Sydney",
    "/usr/share/zoneinfo/Australia/Sydney",
    ":/usr/share/zoneinfo/Australia/Sydney",
    "/usr/share/zoneinfo/posix/Australia/Sydney",
    "../zoneinfo/Australia/Sydney",
    "/etc/localtime",
    "ABC",
    "ABC+",
    "XYZ5",
    "UTC+3",
    "UTC-3",
    "UTC+03",
    "ABC99",
    "ABC0",
    "<ABC>-3",
    "<ABC>1",
    "Etc/GMT+13",
    "Etc/GMT+05",
    "3",
    "+03",
    "-03",
    "12",
    "AB1",
    "UT+3",
    "<AB>1",
    "ABCD-24",
];

const SYSTEM_LIKE: [&str; 25] = [
    "ABC1DEF",
    "AAA3BBB,M3.2.0,M11.1.0",
    "Australia/Sydney,M3.2.0,M11.1.0",
    "<+03>-3",
    "<+3>3",
    "<AB3>1",
    "<123>1",
    "12345",
    "123",
    "1ABC",
    "1:30",
    "UTC+3:30",
    "ABC1:30",
    "ABC100",
    "ABC1 ",
    "A,B",
    "EST,EDT",
    "Australia/Sydney,",
    "America/New_York,",
    "UTC,",
    ",",
    ",UTC",
    "ABC,3",
    "A,B5",
    "XYZ5,",
];

fn tzif(abbreviation: &str, offset: i32, footer: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(offset.to_be_bytes());
    body.extend([0, 0]);
    body.extend(abbreviation.as_bytes());
    body.push(0);
    let header = |version: u8| {
        let mut head = b"TZif".to_vec();
        head.push(version);
        head.extend([0; 15]);
        for count in [
            0,
            0,
            0,
            0,
            1,
            u32::try_from(abbreviation.len() + 1).unwrap_or(0),
        ] {
            head.extend(count.to_be_bytes());
        }
        head
    };
    let mut bytes = header(b'2');
    bytes.extend(&body);
    bytes.extend(header(b'2'));
    bytes.extend(&body);
    bytes.extend(format!("\n{footer}\n").as_bytes());
    bytes
}

#[derive(Clone, Copy)]
pub(super) enum System {
    Pacific,
    Sydney,
    Flat,
    Absent,
}

fn system_file(scratch: &Scratch, system: System) -> Result<PathBuf> {
    let path = scratch.temp().join("system-localtime");
    std::fs::create_dir_all(scratch.temp())?;
    let bytes = match system {
        System::Pacific => tzif("PST", -28800, "PST8PDT,M3.2.0,M11.1.0"),
        System::Sydney => tzif("AEST", 36000, "AEST-10AEDT,M10.1.0,M4.1.0/3"),
        System::Flat => tzif("UTC", 0, "UTC0"),
        System::Absent => return Ok(scratch.temp().join("no-such-localtime")),
    };
    std::fs::write(&path, bytes)?;
    Ok(path)
}

pub(super) struct Clock {
    pub(super) days: i64,
    pub(super) now: Timestamp,
    pub(super) flat: Timestamp,
}

impl Clock {
    fn at(days: i64, now: Timestamp) -> Result<Self> {
        Ok(Self {
            days,
            now,
            flat: now.checked_add(Span::new().try_hours(days * 24)?)?,
        })
    }

    pub(super) fn calendar(&self, zone: &str) -> Result<Timestamp> {
        Ok(self
            .now
            .to_zoned_datetime(TimeZoneDatabase::bundled().get(zone)?)
            .checked_add(Span::new().try_days(self.days)?)?
            .timestamp())
    }

    fn separates(&self, zones: &[&str]) -> Result<bool> {
        let mut all = vec![self.flat];
        for zone in zones {
            all.push(self.calendar(zone)?);
        }
        Ok(all.iter().enumerate().all(|(index, first)| {
            all[index + 1..]
                .iter()
                .all(|second| (first.as_second() - second.as_second()).abs() >= 1800)
        }))
    }

    pub(super) fn pick(zones: &[&str]) -> Result<Self> {
        let now = Timestamp::now();
        for days in 2..400 {
            let mut every_day_around = true;
            for day in [days - 1, days, days + 1] {
                every_day_around &= Self::at(day, now)?.separates(zones)?;
            }
            if every_day_around {
                return Self::at(days, now);
            }
        }
        bail!("no span of days separates UTC from {zones:?}")
    }
}

#[derive(Clone, Copy)]
pub(super) struct Setup<'a> {
    pub(super) zoneinfo: bool,
    pub(super) system: System,
    pub(super) tz: Option<&'a OsStr>,
}

impl Setup<'_> {
    pub(super) const PACIFIC: Self = Self {
        zoneinfo: false,
        system: System::Pacific,
        tz: None,
    };
    pub(super) const HOST_SYDNEY: Self = Self {
        system: System::Sydney,
        ..Self::PACIFIC
    };
    pub(super) const NO_HOST_ZONE: Self = Self {
        system: System::Absent,
        ..Self::PACIFIC
    };
}

pub(super) async fn expiry_for(
    clock: &Clock,
    process: &[(&str, &str)],
    step: Option<&str>,
    setup: Setup<'_>,
) -> Result<Timestamp> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let localtime = system_file(&scratch, setup.system)?
        .to_string_lossy()
        .into_owned();
    let directory = scratch
        .temp()
        .join("zoneinfo")
        .to_string_lossy()
        .into_owned();
    if setup.zoneinfo {
        std::fs::create_dir_all(&directory)?;
        for name in ["localtime", "posixrules"] {
            std::fs::copy(&localtime, format!("{directory}/{name}"))?;
        }
    }
    let mut process_env = vec![("VELNOR_SYSTEM_LOCALTIME", localtime.as_str())];
    if setup.zoneinfo {
        process_env.push(("TZDIR", directory.as_str()));
    }
    process_env.extend_from_slice(process);
    let days = clock.days.to_string();
    let job = step.map(|value| [("TZ", value)]);
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "bundle"),
            ("path", "a.txt"),
            ("retention-days", &days),
        ],
        Env {
            process: &process_env,
            job: job.as_ref().map_or(&[], |pair| pair.as_slice()),
            tz: setup.tz,
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{step:?}: {}", ran.stdout);
    fake.calls("CreateArtifact")?[0]["expires_at"]
        .as_str()
        .context("CreateArtifact sent no expires_at")?
        .parse::<Timestamp>()
        .map_err(Into::into)
}

pub(super) fn assert_near(sent: Timestamp, expected: Timestamp, row: &str, clock: &Clock) {
    let drift = (sent.as_second() - expected.as_second()).abs();
    assert!(
        drift < 120,
        "{row:?}: sent {sent}, expected {expected} (now {}, {} days)",
        clock.now,
        clock.days
    );
}

async fn rows_reach(rows: &[&str], zone: Option<&str>) -> Result<()> {
    let clock = Clock::pick(&[PACIFIC, SYDNEY])?;
    let expected = zone.map_or(Ok(clock.flat), |zone| clock.calendar(zone))?;
    for row in rows {
        let sent = expiry_for(&clock, &[], Some(row), Setup::PACIFIC).await?;
        assert_near(sent, expected, row, &clock);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tz_that_names_a_zone_exactly_adds_days_on_that_calendar() -> Result<()> {
    rows_reach(&NAMED, Some(SYDNEY)).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tz_node_reads_as_a_fixed_offset_or_an_unknown_name_adds_whole_days() -> Result<()> {
    rows_reach(&UTC_LIKE, None).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tz_node_cannot_use_leaves_the_system_zone_in_force() -> Result<()> {
    rows_reach(&SYSTEM_LIKE, Some(PACIFIC)).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_any_tz_the_system_zone_is_in_force() -> Result<()> {
    let clock = Clock::pick(&[PACIFIC, SYDNEY])?;
    let sent = expiry_for(&clock, &[], None, Setup::PACIFIC).await?;
    assert_near(sent, clock.calendar(PACIFIC)?, "unset", &clock);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_process_tz_applies_only_when_the_step_sets_none() -> Result<()> {
    let clock = Clock::pick(&[PACIFIC, SYDNEY])?;
    let (sydney, pacific) = (clock.calendar(SYDNEY)?, clock.calendar(PACIFIC)?);
    let cases = [
        (SYDNEY, None, sydney),
        ("ABC1DEF", None, pacific),
        ("", None, clock.flat),
        (SYDNEY, Some("UTC"), clock.flat),
        (SYDNEY, Some(""), clock.flat),
        ("UTC", Some(SYDNEY), sydney),
        ("UTC", Some("ABC1DEF"), pacific),
    ];
    for (process, step, expected) in cases {
        let sent = expiry_for(&clock, &[("TZ", process)], step, Setup::PACIFIC).await?;
        assert_near(
            sent,
            expected,
            &format!("{process:?} then {step:?}"),
            &clock,
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zoneinfo_file_that_is_not_a_zone_name_is_a_fixed_offset() -> Result<()> {
    let clock = Clock::pick(&[PACIFIC, SYDNEY])?;
    let setup = Setup {
        zoneinfo: true,
        ..Setup::PACIFIC
    };
    for row in ["localtime", "posixrules"] {
        let sent = expiry_for(&clock, &[], Some(row), setup).await?;
        assert_near(sent, clock.flat, row, &clock);
    }
    Ok(())
}
