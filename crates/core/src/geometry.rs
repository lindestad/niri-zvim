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
    let (primary, perpendicular) = match direction {
        Direction::Left if ox < cx => (cx - ox, (cy - oy).abs()),
        Direction::Right if ox > cx => (ox - cx, (cy - oy).abs()),
        Direction::Up if oy < cy => (cy - oy, (cx - ox).abs()),
        Direction::Down if oy > cy => (oy - cy, (cx - ox).abs()),
        _ => return None,
    };

    Some(primary + perpendicular * 4.0)
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
}
