use std::collections::VecDeque;

use niri_zvim_core::Direction;

#[derive(Clone, Copy)]
pub(super) struct PendingNavigation {
    pub(super) sequence: u64,
    pub(super) direction: Direction,
    pub(super) expected: u64,
}

#[derive(Clone, Copy)]
pub(super) struct PendingNiriNavigation {
    pub(super) sequence: u64,
    pub(super) direction: Direction,
    pub(super) expected: Option<u64>,
}

pub(super) fn acknowledge_niri_pending(
    pending: &mut VecDeque<PendingNiriNavigation>,
    acknowledged_sequence: u64,
) {
    while pending
        .front()
        .is_some_and(|navigation| navigation.sequence <= acknowledged_sequence)
    {
        pending.pop_front();
    }
}

pub(super) fn acknowledge_niri_observed(
    pending: &mut VecDeque<PendingNiriNavigation>,
    observed: Option<u64>,
) {
    let Some(position) = pending
        .iter()
        .rposition(|navigation| navigation.expected == observed)
    else {
        return;
    };
    pending.drain(..=position);
}

pub(super) fn acknowledge_pending(
    pending: Option<&mut VecDeque<PendingNavigation>>,
    acknowledged_sequence: u64,
) -> bool {
    let Some(pending) = pending else {
        return false;
    };
    let before = pending.len();
    while pending
        .front()
        .is_some_and(|navigation| navigation.sequence <= acknowledged_sequence)
    {
        pending.pop_front();
    }
    before != pending.len()
}

pub(super) fn acknowledge_observed(
    pending: Option<&mut VecDeque<PendingNavigation>>,
    observed: u64,
) -> bool {
    let Some(pending) = pending else {
        return false;
    };
    let Some(position) = pending
        .iter()
        .rposition(|navigation| navigation.expected == observed)
    else {
        return false;
    };
    pending.drain(..=position);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_acknowledgement_clears_every_covered_prediction() {
        let mut pending = VecDeque::from([
            PendingNavigation {
                sequence: 4,
                direction: Direction::Right,
                expected: 11,
            },
            PendingNavigation {
                sequence: 5,
                direction: Direction::Right,
                expected: 12,
            },
            PendingNavigation {
                sequence: 7,
                direction: Direction::Left,
                expected: 11,
            },
        ]);

        assert!(acknowledge_pending(Some(&mut pending), 5));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().sequence, 7);
        assert!(!acknowledge_pending(Some(&mut pending), 5));
    }

    #[test]
    fn observed_target_acknowledges_the_matching_prediction_prefix() {
        let mut pending = VecDeque::from([
            PendingNavigation {
                sequence: 4,
                direction: Direction::Right,
                expected: 11,
            },
            PendingNavigation {
                sequence: 5,
                direction: Direction::Right,
                expected: 12,
            },
            PendingNavigation {
                sequence: 6,
                direction: Direction::Right,
                expected: 13,
            },
        ]);

        assert!(acknowledge_observed(Some(&mut pending), 12));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().expected, 13);
        assert!(!acknowledge_observed(Some(&mut pending), 12));
    }

    #[test]
    fn niri_action_snapshot_clears_no_op_and_covered_predictions() {
        let mut pending = VecDeque::from([
            PendingNiriNavigation {
                sequence: 4,
                direction: Direction::Up,
                expected: None,
            },
            PendingNiriNavigation {
                sequence: 5,
                direction: Direction::Down,
                expected: Some(12),
            },
            PendingNiriNavigation {
                sequence: 7,
                direction: Direction::Left,
                expected: Some(11),
            },
        ]);

        acknowledge_niri_pending(&mut pending, 5);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().sequence, 7);
    }

    #[test]
    fn niri_focus_event_acknowledges_matching_prediction_prefix() {
        let mut pending = VecDeque::from([
            PendingNiriNavigation {
                sequence: 4,
                direction: Direction::Up,
                expected: Some(10),
            },
            PendingNiriNavigation {
                sequence: 5,
                direction: Direction::Up,
                expected: Some(11),
            },
            PendingNiriNavigation {
                sequence: 6,
                direction: Direction::Left,
                expected: Some(12),
            },
        ]);

        acknowledge_niri_observed(&mut pending, Some(11));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().expected, Some(12));
    }
}
