use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::HEADER_H;

const SAME_COST: f32 = 0.5;
const SIDE_BY_SIDE: f32 = 40.0;
const LANE: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn out(self) -> Vec2 {
        match self {
            Side::Left => vec2(-1.0, 0.0),
            Side::Right => vec2(1.0, 0.0),
            Side::Top => vec2(0.0, -1.0),
            Side::Bottom => vec2(0.0, 1.0),
        }
    }

    fn vertical(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

type Slots = std::collections::HashMap<(usize, Side), Vec<(usize, usize, f32)>>;

pub struct Route {
    pub pts: Vec<Pos2>,
    pub from_u: Vec2,
    pub to_u: Vec2,
}

fn sides(a: Rect, b: Rect) -> (Side, Side) {
    let room = 40.0;
    if a.max.x + room <= b.min.x {
        (Side::Right, Side::Left)
    } else if b.max.x + room <= a.min.x {
        (Side::Left, Side::Right)
    } else if a.max.y + room <= b.min.y {
        (Side::Bottom, Side::Top)
    } else if b.max.y + room <= a.min.y {
        (Side::Top, Side::Bottom)
    } else {
        (Side::Right, Side::Right)
    }
}

pub fn route_links(ends: &[Option<(usize, usize)>], boxes: &[Rect]) -> Vec<Option<Route>> {
    let mut slots: Slots = Default::default();
    let mut chosen = vec![None; ends.len()];
    for (i, end) in ends.iter().enumerate() {
        let Some((a, b)) = *end else { continue };
        let (sa, sb) = if a == b {
            (Side::Right, Side::Right)
        } else {
            sides(boxes[a], boxes[b])
        };
        chosen[i] = Some((sa, sb));
        let along = |r: Rect, s: Side| {
            if s.vertical() {
                r.center().y
            } else {
                r.center().x
            }
        };
        slots
            .entry((a, sa))
            .or_default()
            .push((i, 0, along(boxes[b], sa)));
        slots.entry((b, sb)).or_default().push((
            i,
            1,
            along(boxes[a], sb) + if a == b { 1.0 } else { 0.0 },
        ));
    }
    let mut at = vec![[Pos2::ZERO; 2]; ends.len()];
    for ((bx, side), list) in &mut slots {
        list.sort_by(|x, y| x.2.total_cmp(&y.2).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));
        let r = boxes[*bx];
        let n = list.len() as f32;
        for (k, &(i, end, _)) in list.iter().enumerate() {
            let t = (k as f32 + 1.0) / (n + 1.0);
            at[i][end] = match side {
                Side::Left | Side::Right => {
                    let top = (r.min.y + HEADER_H).min(r.max.y);
                    let y = top + (r.max.y - top) * t;
                    pos2(
                        if *side == Side::Left {
                            r.min.x
                        } else {
                            r.max.x
                        },
                        y,
                    )
                }
                Side::Top | Side::Bottom => {
                    let x = r.min.x + r.width() * t;
                    pos2(x, if *side == Side::Top { r.min.y } else { r.max.y })
                }
            };
        }
    }
    let mut routes: Vec<Option<Route>> = Vec::with_capacity(ends.len());
    for (i, end) in ends.iter().enumerate() {
        let (Some((a, b)), Some((sa, sb))) = (*end, chosen[i]) else {
            routes.push(None);
            continue;
        };
        let (p, q) = (at[i][0], at[i][1]);
        let obstacles: Vec<Rect> = boxes
            .iter()
            .enumerate()
            .filter(|&(k, _)| k != a && k != b)
            .map(|(_, r)| r.expand(4.0))
            .collect();
        let pts = match (sa, sb) {
            (Side::Right, Side::Right) => {
                let x = boxes[a].max.x.max(boxes[b].max.x) + 30.0;
                vec![p, pos2(x, p.y), pos2(x, q.y), q]
            }
            (s, _) if s.vertical() => best_path(p, q, &obstacles),
            _ => {
                let t = |p: Pos2| pos2(p.y, p.x);
                let flipped: Vec<Rect> = obstacles
                    .iter()
                    .map(|r| Rect::from_min_max(t(r.min), t(r.max)))
                    .collect();
                best_path(t(p), t(q), &flipped).into_iter().map(t).collect()
            }
        };
        routes.push(Some(Route {
            pts,
            from_u: sa.out(),
            to_u: sb.out(),
        }));
    }
    spread_side_by_side(&mut routes);
    routes
}

struct Middle {
    route: usize,
    vertical: bool,
    at: f32,
}

fn middle(route: usize, pts: &[Pos2]) -> Option<Middle> {
    if pts.len() != 4 {
        return None;
    }
    let vertical = pts[1].x == pts[2].x;
    let at = if vertical { pts[1].x } else { pts[1].y };
    Some(Middle {
        route,
        vertical,
        at,
    })
}

fn spread_side_by_side(routes: &mut [Option<Route>]) {
    let mut middles: Vec<Middle> = routes
        .iter()
        .enumerate()
        .filter_map(|(i, route)| middle(i, &route.as_ref()?.pts))
        .collect();
    middles.sort_by(|a, b| a.vertical.cmp(&b.vertical).then(a.at.total_cmp(&b.at)));
    let side_by_side =
        |a: &Middle, b: &Middle| a.vertical == b.vertical && b.at - a.at < SIDE_BY_SIDE;
    for group in middles.chunk_by(side_by_side) {
        spread(routes, group);
    }
}

fn spread(routes: &mut [Option<Route>], group: &[Middle]) {
    let mut members: Vec<usize> = group.iter().map(|m| m.route).collect();
    // Link order, not position: a line passing another keeps its lane instead of both jumping.
    members.sort();
    let n = members.len() as f32;
    for (k, i) in members.into_iter().enumerate() {
        let shift = (k as f32 - (n - 1.0) / 2.0) * LANE;
        if let Some(route) = &mut routes[i] {
            for p in &mut route.pts[1..3] {
                if group[0].vertical {
                    p.x += shift;
                } else {
                    p.y += shift;
                }
            }
        }
    }
}

fn best_path(p: Pos2, q: Pos2, obstacles: &[Rect]) -> Vec<Pos2> {
    let candidates = candidate_paths(p, q, obstacles);
    let fewest_hits = candidates
        .iter()
        .map(|path| obstacle_hits(path, obstacles))
        .min()
        .unwrap();
    let least_blocked: Vec<Vec<Pos2>> = candidates
        .into_iter()
        .filter(|path| obstacle_hits(path, obstacles) == fewest_hits)
        .collect();
    let cheapest = least_blocked
        .iter()
        .map(|path| path_cost(path))
        .fold(f32::INFINITY, f32::min);
    least_blocked
        .into_iter()
        .find(|path| path_cost(path) < cheapest + SAME_COST)
        .unwrap()
}

fn candidate_paths(p: Pos2, q: Pos2, obstacles: &[Rect]) -> Vec<Vec<Pos2>> {
    let d = if q.x >= p.x { 1.0 } else { -1.0 };
    let (x1, x2) = (p.x + d * 22.0, q.x - d * 22.0);
    let mut candidates: Vec<Vec<Pos2>> = [(p.x + q.x) / 2.0, x1, x2]
        .into_iter()
        .map(|x| vec![p, pos2(x, p.y), pos2(x, q.y), q])
        .collect();
    let (lo, hi) = (p.x.min(q.x), p.x.max(q.x));
    for r in obstacles.iter().filter(|r| r.max.x > lo && r.min.x < hi) {
        for y in [r.min.y - 10.0, r.max.y + 10.0] {
            candidates.push(vec![
                p,
                pos2(x1, p.y),
                pos2(x1, y),
                pos2(x2, y),
                pos2(x2, q.y),
                q,
            ]);
        }
    }
    candidates
}

fn obstacle_hits(path: &[Pos2], obstacles: &[Rect]) -> usize {
    path.windows(2)
        .map(|w| obstacles.iter().filter(|r| crosses(w[0], w[1], r)).count())
        .sum()
}

fn path_cost(path: &[Pos2]) -> f32 {
    let length: f32 = path.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    length + path.len() as f32 * 30.0
}

fn crosses(u: Pos2, v: Pos2, r: &Rect) -> bool {
    let (x0, x1) = (u.x.min(v.x), u.x.max(v.x));
    let (y0, y1) = (u.y.min(v.y), u.y.max(v.y));
    x0 < r.max.x && x1 > r.min.x && y0 < r.max.y && y1 > r.min.y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_links_bend_halfway() {
        for step in 0..200 {
            let shift = step as f32 * 3.7;
            let p = pos2(150.3 + shift, 80.1);
            let q = pos2(420.9 + shift * 1.3, 260.7 + shift);
            let path = best_path(p, q, &[]);
            assert_eq!(path[1].x, (p.x + q.x) / 2.0, "from {p:?} to {q:?}");
        }
    }

    #[test]
    fn a_blocked_middle_moves_the_bend_next_to_the_start() {
        let (p, q) = (pos2(0.0, 0.0), pos2(300.0, 100.0));
        let middle = Rect::from_min_max(pos2(140.0, 30.0), pos2(160.0, 70.0));
        let path = best_path(p, q, &[middle]);
        assert_eq!(path[1].x, p.x + 22.0);
    }

    #[test]
    fn close_lines_spread_apart_wherever_they_are() {
        for step in 0..200 {
            let x = step as f32 * 0.25;
            let mut routes = vec![bent_at(x), bent_at(x + 3.0), bent_at(x + 100.0)];
            spread_side_by_side(&mut routes);
            assert_eq!(bends(&routes), [x - 4.5, x + 7.5, x + 100.0], "at {x}");
        }
    }

    fn bent_at(x: f32) -> Option<Route> {
        Some(Route {
            pts: vec![
                pos2(-50.0, 0.0),
                pos2(x, 0.0),
                pos2(x, 100.0),
                pos2(250.0, 100.0),
            ],
            from_u: Side::Right.out(),
            to_u: Side::Left.out(),
        })
    }

    fn bends(routes: &[Option<Route>]) -> Vec<f32> {
        routes.iter().flatten().map(|r| r.pts[1].x).collect()
    }
}
