use super::steps::{Idle, idle};
use crate::launch_harness::{assigned_wait, available};
use velnor_runner_github::Poll;

#[test]
fn busy_capacity_keeps_new_assignments_and_rejects_ambiguous_offers() {
    let polls = [
        assigned_wait(11, 1),
        available(&[4]),
        assigned_wait(12, 0),
        Poll::Empty,
        available(&[4, 5]),
    ];
    let classes = [
        Idle::Scale,
        Idle::Launch,
        Idle::Ack,
        Idle::Empty,
        Idle::Blocked,
    ];

    for (polled, expected) in polls.iter().zip(classes) {
        assert_eq!(idle(polled), expected);
    }
    assert_eq!(idle(&polls[0]), Idle::Scale);
    assert_eq!(idle(&polls[4]), Idle::Blocked);
}
