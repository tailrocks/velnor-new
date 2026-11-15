// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod io;
#[cfg(test)]
mod tests;

pub use io::show_reservations;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservationRow {
    pub reservation_id: i64,
    pub connection_id: String,
    pub reservation_state: String,
    pub intent_state: String,
    pub create_dispatch_state: Option<String>,
    pub create_state: Option<String>,
    pub start_state: Option<String>,
    pub abort_state: Option<String>,
    pub helper_id: Option<String>,
    pub abort_helper_id: Option<String>,
}

impl ReservationRow {
    #[must_use]
    pub fn render(&self) -> String {
        let text = |value: &Option<String>| value.clone().unwrap_or_else(|| "-".to_owned());
        format!(
            "reservation={} connection={} reservation_state={} intent_state={} create_dispatch_state={} create_state={} start_state={} abort_state={} helper_id={} abort_helper_id={}",
            self.reservation_id,
            self.connection_id,
            self.reservation_state,
            self.intent_state,
            text(&self.create_dispatch_state),
            text(&self.create_state),
            text(&self.start_state),
            text(&self.abort_state),
            text(&self.helper_id),
            text(&self.abort_helper_id),
        )
    }
}
