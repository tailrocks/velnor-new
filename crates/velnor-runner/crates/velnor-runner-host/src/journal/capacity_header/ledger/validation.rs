use super::super::model::{
    GenerationState, HeaderRecord, MAX_HEADER_CAPACITY, MAX_SESSION_ID_BYTES, PollPurpose,
};

pub(super) fn valid_record(record: &HeaderRecord) -> bool {
    let sequence = record.attempt_sequence;
    let in_flight = record.in_flight_poll;
    let delete = record.in_flight_delete;
    let sequence_valid = match record.state {
        GenerationState::Pending => match in_flight {
            Some(_) => sequence > 0,
            None => sequence == 0,
        },
        GenerationState::Observed => sequence > 0,
        GenerationState::Uncertain => sequence > 0 && in_flight.is_none(),
        GenerationState::Released => {
            sequence > 0 && in_flight.is_none() && record.zero_header_settled
        }
    };
    let zero_poll_valid = !record.zero_header_settled
        || matches!(
            record.state,
            GenerationState::Observed | GenerationState::Released
        ) && in_flight.is_none();
    let in_flight_valid = in_flight.is_none_or(|poll| {
        poll.sequence == sequence
            && poll.header_value <= MAX_HEADER_CAPACITY
            && match poll.purpose {
                PollPurpose::Advertise => poll.header_value == record.target,
                PollPurpose::ZeroHeader => poll.header_value == 0 && !record.zero_header_settled,
            }
    });
    let delete_valid = match (record.delete_sequence, delete, record.delete_uncertain) {
        (0, None, false) => true,
        (sequence, Some(attempt), false) => {
            sequence > 0
                && attempt.sequence == sequence
                && record.state == GenerationState::Observed
                && record.zero_header_settled
                && in_flight.is_none()
        }
        (sequence, None, true) => {
            sequence > 0 && record.state == GenerationState::Observed && record.zero_header_settled
        }
        (sequence, None, false) => {
            sequence > 0 && record.state == GenerationState::Released && record.zero_header_settled
        }
        _ => false,
    };
    record.identity.scale_set_id > 0
        && !record.identity.session_id.0.is_empty()
        && record.identity.session_id.0.len() <= MAX_SESSION_ID_BYTES
        && !record.identity.session_id.0.chars().any(char::is_control)
        && record.target <= MAX_HEADER_CAPACITY
        && record.generation.0 > 0
        && sequence_valid
        && zero_poll_valid
        && in_flight_valid
        && delete_valid
}
