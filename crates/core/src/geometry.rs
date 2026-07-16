use std::collections::BTreeMap;

use crate::{Direction, NeighborMap};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

pub fn directional_neighbors<I>(rectangles: I) -> BTreeMap<u64, NeighborMap<u64>>
where
    I: IntoIterator<Item = (u64, Rect)>,
{
    let rectangles: Vec<_> = rectangles.into_iter().collect();
    rectangles
        .iter()
        .map(|(id, rect)| {
            let mut neighbors = NeighborMap::default();
            for direction in Direction::ALL {
                let candidate = rectangles
                    .iter()
                    .filter(|(other_id, _)| other_id != id)
                    .filter_map(|(other_id, other)| {
                        score(*rect, *other, direction).map(|score| (score, *other_id))
                    })
                    .min_by(|(a, aid), (b, bid)| a.total_cmp(b).then_with(|| aid.cmp(bid)))
                    .map(|(_, id)| id);
                neighbors.set(direction, candidate);
            }
            (*id, neighbors)
        })
        .collect()
}

fn score(current: Rect, candidate: Rect, direction: Direction) -> Option<f64> {
    let (cx, cy) = current.center();
    let (ox, oy) = candidate.center();
    let current_right = current.x + current.width;
    let current_bottom = current.y + current.height;
    let candidate_right = candidate.x + candidate.width;
    let candidate_bottom = candidate.y + candidate.height;
    let (primary, perpendicular) = match direction {
        Direction::Left
            if candidate_right <= current.x
                && overlaps(current.y, current_bottom, candidate.y, candidate_bottom) =>
        {
            (current.x - candidate_right, (cy - oy).abs())
        }
        Direction::Right
            if candidate.x >= current_right
                && overlaps(current.y, current_bottom, candidate.y, candidate_bottom) =>
        {
            (candidate.x - current_right, (cy - oy).abs())
        }
        Direction::Up
            if candidate_bottom <= current.y
                && overlaps(current.x, current_right, candidate.x, candidate_right) =>
        {
            (current.y - candidate_bottom, (cx - ox).abs())
        }
        Direction::Down
            if candidate.y >= current_bottom
                && overlaps(current.x, current_right, candidate.x, candidate_right) =>
        {
            (candidate.y - current_bottom, (cx - ox).abs())
        }
        _ => return None,
    };

    Some(primary + perpendicular * 4.0)
}

fn overlaps(first_start: f64, first_end: f64, second_start: f64, second_end: f64) -> bool {
    first_start < second_end && second_start < first_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_three_column_layout() {
        let graph = directional_neighbors([
            (
                1,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
            (
                2,
                Rect {
                    x: 100.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
            (
                3,
                Rect {
                    x: 200.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
        ]);
        assert_eq!(graph[&1].right, Some(2));
        assert_eq!(graph[&2].left, Some(1));
        assert_eq!(graph[&2].right, Some(3));
        assert_eq!(graph[&3].right, None);
    }

    #[test]
    fn full_height_pane_has_no_vertical_neighbor_in_adjacent_stack() {
        let graph = directional_neighbors([
            (
                1,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 25.0,
                },
            ),
            (
                2,
                Rect {
                    x: 0.0,
                    y: 25.0,
                    width: 50.0,
                    height: 25.0,
                },
            ),
            (
                3,
                Rect {
                    x: 0.0,
                    y: 50.0,
                    width: 50.0,
                    height: 50.0,
                },
            ),
            (
                4,
                Rect {
                    x: 50.0,
                    y: 0.0,
                    width: 50.0,
                    height: 100.0,
                },
            ),
        ]);

        assert_eq!(graph[&4].up, None);
        assert_eq!(graph[&4].down, None);
        assert!(matches!(graph[&4].left, Some(1 | 2 | 3)));
        assert_eq!(graph[&1].right, Some(4));
        assert_eq!(graph[&2].right, Some(4));
        assert_eq!(graph[&3].right, Some(4));
    }

    #[test]
    fn gaps_between_tiled_rectangles_still_form_neighbors() {
        let graph = directional_neighbors([
            (
                1,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 40.0,
                    height: 40.0,
                },
            ),
            (
                2,
                Rect {
                    x: 50.0,
                    y: 0.0,
                    width: 40.0,
                    height: 40.0,
                },
            ),
        ]);

        assert_eq!(graph[&1].right, Some(2));
        assert_eq!(graph[&2].left, Some(1));
    }
}
