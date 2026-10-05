//! Fractional indexing for list and card ordering.

/// Gap between consecutive positions when appending or rebalancing.
pub const POSITION_STEP: f64 = 1000.0;
/// Neighbors closer than this are renumbered instead of split, long before
/// f64 midpoints stop being distinct.
const MIN_POSITION_GAP: f64 = 0.001;

/// An ordered row competing for a slot: the other lists on a board or cards in a list.
#[derive(Debug, Clone)]
pub struct Sibling {
    pub id: String,
    pub position: f64,
}

/// Where the moved item lands and which siblings must be renumbered.
#[derive(Debug, PartialEq)]
pub struct Reposition<'a> {
    pub position: f64,
    /// Empty unless the neighbors were too close and the whole sequence is renumbered.
    pub rebalanced: Vec<(&'a str, f64)>,
}

/// Slot the moved item takes among `siblings` (which exclude it): directly after
/// `after_id`, or first when `None`. Returns `None` if `after_id` is not a sibling.
pub fn slot_after(siblings: &[Sibling], after_id: Option<&str>) -> Option<usize> {
    match after_id {
        None => Some(0),
        Some(after_id) => siblings
            .iter()
            .position(|s| s.id == after_id)
            .map(|i| i + 1),
    }
}

/// Positions the moved item at `slot` (0..=siblings.len()) among `siblings`,
/// which are ordered by position and exclude the moved item.
pub fn reposition(siblings: &[Sibling], slot: usize) -> Reposition<'_> {
    let prev = slot
        .checked_sub(1)
        .and_then(|i| siblings.get(i))
        .map(|s| s.position);
    let next = siblings.get(slot).map(|s| s.position);

    let (low, high) = match (prev, next) {
        (None, None) => return unchanged(POSITION_STEP),
        (Some(prev), None) => return unchanged(prev + POSITION_STEP),
        // At the start the lower neighbor is 0, so the midpoint is first / 2.
        (None, Some(next)) => (0.0, next),
        (Some(prev), Some(next)) => (prev, next),
    };
    if high - low >= MIN_POSITION_GAP {
        return unchanged((low + high) / 2.0);
    }

    let rebalanced = siblings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let new_slot = if i < slot { i } else { i + 1 };
            (s.id.as_str(), step_position(new_slot))
        })
        .collect();
    Reposition {
        position: step_position(slot),
        rebalanced,
    }
}

fn unchanged(position: f64) -> Reposition<'static> {
    Reposition {
        position,
        rebalanced: Vec::new(),
    }
}

/// 1000.0, 2000.0, 3000.0 ... for slots 0, 1, 2 ...
fn step_position(slot: usize) -> f64 {
    (slot + 1) as f64 * POSITION_STEP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn siblings(positions: &[(&str, f64)]) -> Vec<Sibling> {
        positions
            .iter()
            .map(|(id, position)| Sibling {
                id: id.to_string(),
                position: *position,
            })
            .collect()
    }

    #[test]
    fn first_item_starts_at_step() {
        assert_eq!(reposition(&[], 0), unchanged(1000.0));
    }

    #[test]
    fn end_start_and_between() {
        let s = siblings(&[("a", 1000.0), ("b", 2000.0)]);
        assert_eq!(reposition(&s, 2).position, 3000.0);
        assert_eq!(reposition(&s, 0).position, 500.0);
        assert_eq!(reposition(&s, 1).position, 1500.0);
        assert!(reposition(&s, 1).rebalanced.is_empty());
    }

    #[test]
    fn tight_gap_rebalances_every_sibling_around_the_slot() {
        let s = siblings(&[("a", 1000.0), ("b", 1000.0005), ("c", 1001.0)]);
        assert_eq!(
            reposition(&s, 1),
            Reposition {
                position: 2000.0,
                rebalanced: vec![("a", 1000.0), ("b", 3000.0), ("c", 4000.0)],
            }
        );
    }

    #[test]
    fn tight_gap_at_start_rebalances() {
        let s = siblings(&[("a", 0.0005)]);
        assert_eq!(
            reposition(&s, 0),
            Reposition {
                position: 1000.0,
                rebalanced: vec![("a", 2000.0)],
            }
        );
    }

    #[test]
    fn slot_after_finds_anchor_or_rejects_unknown() {
        let s = siblings(&[("a", 1000.0), ("b", 2000.0)]);
        assert_eq!(slot_after(&s, None), Some(0));
        assert_eq!(slot_after(&s, Some("a")), Some(1));
        assert_eq!(slot_after(&s, Some("b")), Some(2));
        assert_eq!(slot_after(&s, Some("zzz")), None);
    }
}
