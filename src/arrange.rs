use crate::model::{Card, Link, Table};
use eframe::egui::{Pos2, Vec2, pos2};

const GAP_X: f32 = 170.0;
const GAP_Y: f32 = 50.0;

/// Arrange puts the start of an edge left of its end. Starting at the "one" side puts a
/// parent left of its children, however the link is written.
pub fn edges_of<'a>(
    tables: &[Table],
    links: impl IntoIterator<Item = &'a Link>,
) -> Vec<(usize, usize)> {
    let index = |name: &str| tables.iter().position(|t| t.name == name);
    links
        .into_iter()
        .filter_map(|link| {
            let [start, end] = one_side_first(link);
            Some((index(start)?, index(end)?))
        })
        .collect()
}

fn one_side_first(link: &Link) -> [&str; 2] {
    let many = |card| matches!(card, Card::OneOrMore | Card::ZeroOrMore);
    if many(link.from_card) && !many(link.to_card) {
        [&link.to, &link.from]
    } else {
        [&link.from, &link.to]
    }
}

pub fn arrange(sizes: &[Vec2], links: &[(usize, usize)]) -> Vec<Pos2> {
    let n = sizes.len();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for &(a, b) in links {
        if a != b && a < n && b < n && !edges.contains(&(a, b)) && !edges.contains(&(b, a)) {
            edges.push((a, b));
        }
    }
    let edges = without_cycles(n, &edges);
    let layer = layers(n, &edges);
    let depth = layer.iter().max().map_or(0, |m| m + 1);
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for v in 0..n {
        columns[layer[v]].push(v);
    }
    order(&mut columns, &edges, &layer);

    let mut x = 0.0;
    let mut pos = vec![Pos2::ZERO; n];
    for col in &columns {
        let width = col.iter().map(|&v| sizes[v].x).fold(0.0, f32::max);
        let mut y = 0.0;
        for &v in col {
            pos[v] = pos2(x, y);
            y += sizes[v].y + GAP_Y;
        }
        x += width + GAP_X;
    }
    for _ in 0..6 {
        for col in &columns {
            settle(col, &mut pos, sizes, &edges);
        }
    }
    let min_y = pos.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    pos.iter().map(|p| pos2(p.x, p.y - min_y)).collect()
}

fn without_cycles(n: usize, edges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    fn visit(v: usize, edges: &[(usize, usize)], mark: &mut [Mark], back: &mut Vec<usize>) {
        mark[v] = Mark::Open;
        for (i, &(a, b)) in edges.iter().enumerate() {
            if a != v {
                continue;
            }
            match mark[b] {
                Mark::New => visit(b, edges, mark, back),
                Mark::Open => back.push(i),
                Mark::Done => {}
            }
        }
        mark[v] = Mark::Done;
    }
    let mut mark = vec![Mark::New; n];
    let mut back = Vec::new();
    for v in 0..n {
        if mark[v] == Mark::New {
            visit(v, edges, &mut mark, &mut back);
        }
    }
    edges
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| if back.contains(&i) { (b, a) } else { (a, b) })
        .collect()
}

fn layers(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut layer = vec![0usize; n];
    for _ in 0..n {
        for &(a, b) in edges {
            layer[b] = layer[b].max(layer[a] + 1);
        }
    }
    for v in 0..n {
        let has_pred = edges.iter().any(|&(_, b)| b == v);
        let succ = edges
            .iter()
            .filter(|&&(a, _)| a == v)
            .map(|&(_, b)| layer[b])
            .min();
        if !has_pred && let Some(s) = succ {
            layer[v] = s.saturating_sub(1);
        }
    }
    layer
}

fn neighbours(v: usize, edges: &[(usize, usize)]) -> impl Iterator<Item = usize> + '_ {
    edges.iter().filter_map(move |&(a, b)| {
        if a == v {
            Some(b)
        } else if b == v {
            Some(a)
        } else {
            None
        }
    })
}

fn order(columns: &mut [Vec<usize>], edges: &[(usize, usize)], layer: &[usize]) {
    let rank = |columns: &[Vec<usize>]| {
        let mut r = vec![0.0f32; layer.len()];
        for col in columns {
            for (i, &v) in col.iter().enumerate() {
                r[v] = (i as f32 + 0.5) / col.len() as f32;
            }
        }
        r
    };
    let mut best = columns.to_vec();
    let mut best_crossings = crossings(columns, edges, layer);
    for round in 0..24 {
        let down = round % 2 == 0;
        let ids: Vec<usize> = if down {
            (0..columns.len()).collect()
        } else {
            (0..columns.len()).rev().collect()
        };
        for l in ids {
            let r = rank(columns);
            let keys: Vec<(usize, f32)> = columns[l]
                .iter()
                .map(|&v| {
                    let side: Vec<f32> = neighbours(v, edges)
                        .filter(|&u| if down { layer[u] < l } else { layer[u] > l })
                        .map(|u| r[u])
                        .collect();
                    let key = if side.is_empty() {
                        r[v]
                    } else {
                        side.iter().sum::<f32>() / side.len() as f32
                    };
                    (v, key)
                })
                .collect();
            let mut keys = keys;
            keys.sort_by(|a, b| a.1.total_cmp(&b.1));
            columns[l] = keys.into_iter().map(|(v, _)| v).collect();
        }
        let c = crossings(columns, edges, layer);
        if c < best_crossings {
            best_crossings = c;
            best = columns.to_vec();
        }
    }
    columns.clone_from_slice(&best);
}

pub fn crossings(columns: &[Vec<usize>], edges: &[(usize, usize)], layer: &[usize]) -> usize {
    let mut at = vec![0.0f32; layer.len()];
    for col in columns {
        for (i, &v) in col.iter().enumerate() {
            at[v] = (i as f32 + 0.5) / col.len() as f32;
        }
    }
    let seg = |&(a, b): &(usize, usize)| {
        let (a, b) = if layer[a] <= layer[b] { (a, b) } else { (b, a) };
        (layer[a] as f32, at[a], layer[b] as f32, at[b])
    };
    let mut count = 0;
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            let (x1, y1, x2, y2) = seg(&edges[i]);
            let (x3, y3, x4, y4) = seg(&edges[j]);
            let lo = x1.max(x3);
            let hi = x2.min(x4);
            if hi <= lo {
                continue;
            }
            let y_at =
                |xa: f32, ya: f32, xb: f32, yb: f32, x: f32| ya + (yb - ya) * (x - xa) / (xb - xa);
            let d_lo = y_at(x1, y1, x2, y2, lo) - y_at(x3, y3, x4, y4, lo);
            let d_hi = y_at(x1, y1, x2, y2, hi) - y_at(x3, y3, x4, y4, hi);
            if d_lo * d_hi < 0.0 {
                count += 1;
            }
        }
    }
    count
}

fn settle(col: &[usize], pos: &mut [Pos2], sizes: &[Vec2], edges: &[(usize, usize)]) {
    let center = |v: usize, pos: &[Pos2]| pos[v].y + sizes[v].y / 2.0;
    let wanted: Vec<f32> = col
        .iter()
        .map(|&v| {
            let ys: Vec<f32> = neighbours(v, edges).map(|u| center(u, pos)).collect();
            if ys.is_empty() {
                center(v, pos)
            } else {
                ys.iter().sum::<f32>() / ys.len() as f32
            }
        })
        .collect();
    let mut tops: Vec<f32> = Vec::with_capacity(col.len());
    let mut floor = f32::NEG_INFINITY;
    for (i, &v) in col.iter().enumerate() {
        let top = (wanted[i] - sizes[v].y / 2.0).max(floor);
        tops.push(top);
        floor = top + sizes[v].y + GAP_Y;
    }
    let shift = col
        .iter()
        .enumerate()
        .map(|(i, &v)| tops[i] + sizes[v].y / 2.0 - wanted[i])
        .sum::<f32>()
        / col.len().max(1) as f32;
    for (i, &v) in col.iter().enumerate() {
        pos[v].y = tops[i] - shift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid;
    use crate::model::Diagram;
    use eframe::egui::{Rect, vec2};

    #[test]
    fn library_has_no_overlaps_and_reads_left_to_right() {
        let blocks = mermaid::blocks(include_str!("../tests/fixtures/library.md"));
        let d = &blocks[1].diagram;
        let pos = arranged_without_overlaps(d);
        let x = |n: &str| pos[index(d, n)].x;
        for pair in ["book", "copy", "loan", "fine"].windows(2) {
            assert!(x(pair[0]) < x(pair[1]), "{} {}", pair[0], pair[1]);
        }
        assert!(x("member") < x("loan"));
        assert!(x("author") < x("book_author"));
        assert!(x("payment") < x("fine"));
    }

    #[test]
    fn links_start_at_their_one_side_or_as_written() {
        for (link, left, right) in [
            ("customer ||--o{ order", "customer", "order"),
            ("order }o--|| customer", "customer", "order"),
            ("item }|--o| order", "order", "item"),
            ("member ||--o| card", "member", "card"),
            ("card |o--|| member", "card", "member"),
            ("tag }o--o{ post", "tag", "post"),
            ("post }|--|{ tag", "post", "tag"),
        ] {
            let d = mermaid::parse(&format!("erDiagram\n{link} : x\n"));
            let pos = arranged_without_overlaps(&d);
            let x = |n: &str| pos[index(&d, n)].x;
            assert!(x(left) < x(right), "{link}");
        }
    }

    #[test]
    fn shop_and_erp_have_no_overlaps() {
        let shop = mermaid::blocks(include_str!("../tests/fixtures/shop.md"));
        arranged_without_overlaps(&shop[0].diagram);
        let erp = mermaid::blocks(include_str!("../tests/fixtures/erp.md"));
        arranged_without_overlaps(&erp[0].diagram);
    }

    #[test]
    fn cycles_do_not_hang() {
        let sizes = vec![vec2(100.0, 50.0); 3];
        let pos = arrange(&sizes, &[(0, 1), (1, 2), (2, 0)]);
        assert_eq!(pos.len(), 3);
    }

    fn index(d: &Diagram, name: &str) -> usize {
        d.tables.iter().position(|t| t.name == name).unwrap()
    }

    fn arranged_without_overlaps(d: &Diagram) -> Vec<Pos2> {
        let sizes: Vec<Vec2> = d
            .tables
            .iter()
            .map(|t| vec2(200.0, 30.0 + 19.0 * t.columns.len() as f32))
            .collect();
        let pos = arrange(&sizes, &edges_of(&d.tables, &d.links));
        let rects: Vec<Rect> = pos
            .iter()
            .zip(&sizes)
            .map(|(p, s)| Rect::from_min_size(*p, *s))
            .collect();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(
                    !rects[i].intersects(rects[j]),
                    "{} overlaps {}",
                    d.tables[i].name,
                    d.tables[j].name
                );
            }
        }
        pos
    }
}
