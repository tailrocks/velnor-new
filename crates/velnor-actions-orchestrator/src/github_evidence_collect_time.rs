//! UTC-second window selection without a general date/time dependency.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
pub(super) struct EvidenceWindow {
    pub(super) captured_at: i64,
    pub(super) start: i64,
    pub(super) end: i64,
}

impl EvidenceWindow {
    pub(super) fn new(window_days: u8) -> Result<Self, &'static str> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "github_evidence_clock")?
            .as_secs();
        let end = i64::try_from(now).map_err(|_| "github_evidence_clock")?;
        let start = end
            .checked_sub(i64::from(window_days) * 86_400)
            .ok_or("github_evidence_clock")?;
        Ok(Self {
            captured_at: end,
            start,
            end,
        })
    }

    pub(super) fn captured_at_text(self) -> String {
        utc(self.captured_at)
    }

    pub(super) fn start_text(self) -> String {
        utc(self.start)
    }

    pub(super) fn end_text(self) -> String {
        utc(self.end)
    }

    pub(super) fn query_end_text(self) -> String {
        utc(self.end.saturating_sub(1))
    }

    pub(super) fn predecessor_cutoff_text(self) -> String {
        utc(self.start.saturating_sub(1))
    }
}

fn utc(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = seconds % 3_600 / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
#[path = "github_evidence_collect_time_tests.rs"]
mod tests;
