use crate::model::{Card, Column, Diagram, Link, Table};

pub struct Block {
    pub title: String,
    pub diagram: Diagram,
}

pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if let Some(h) = trimmed.strip_prefix('#') {
            heading = h.trim_start_matches('#').trim().to_string();
            continue;
        }
        if trimmed != "```mermaid" {
            continue;
        }
        let body: Vec<&str> = lines.by_ref().take_while(|l| l.trim() != "```").collect();
        let body = body.join("\n");
        if body.trim_start().starts_with("erDiagram") {
            let title = if heading.is_empty() {
                "diagram".to_string()
            } else {
                heading.clone()
            };
            out.push(Block {
                title,
                diagram: parse(&body),
            });
        }
    }
    let titles: Vec<String> = out.iter().map(|b| b.title.clone()).collect();
    for (i, block) in out.iter_mut().enumerate() {
        let same = titles.iter().filter(|t| **t == titles[i]).count();
        if same > 1 {
            let nth = titles[..=i].iter().filter(|t| **t == titles[i]).count();
            block.title = format!("{} ({nth})", titles[i]);
        }
    }
    out
}

pub fn parse(src: &str) -> Diagram {
    let mut d = Diagram::default();
    let mut current: Option<Table> = None;
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") || line == "erDiagram" {
            continue;
        }
        if let Some(table) = current.as_mut() {
            if line == "}" {
                d.tables.push(current.take().unwrap());
            } else if let Some(col) = parse_column(line) {
                table.columns.push(col);
            }
            continue;
        }
        if let Some(name) = line.strip_suffix('{') {
            let name = entity_name(name.trim());
            current = Some(Table {
                name,
                ..Default::default()
            });
        } else if let Some(link) = parse_link(line) {
            for name in [&link.from, &link.to] {
                ensure_table(&mut d, name);
            }
            d.links.push(link);
        } else {
            ensure_table(&mut d, &entity_name(line));
        }
    }
    if let Some(table) = current {
        d.tables.push(table);
    }
    d
}

fn ensure_table(d: &mut Diagram, name: &str) {
    if !name.is_empty() && !d.tables.iter().any(|t| t.name == name) {
        d.tables.push(Table {
            name: name.to_string(),
            ..Default::default()
        });
    }
}

fn entity_name(s: &str) -> String {
    let s = s.split('[').next().unwrap_or(s).trim();
    s.trim_matches('"').to_string()
}

fn parse_column(line: &str) -> Option<Column> {
    let (head, note) = match line.find('"') {
        Some(i) => {
            let rest = &line[i + 1..];
            let note = rest.strip_suffix('"').unwrap_or(rest);
            (&line[..i], Some(note.to_string()))
        }
        None => (line, None),
    };
    let mut words = head.split_whitespace();
    let ty = words.next()?.to_string();
    let name = words.next()?.to_string();
    let keys = words.collect::<Vec<_>>().join(" ");
    let keys: Vec<&str> = keys.split(',').map(str::trim).collect();
    Some(Column {
        name,
        ty,
        pk: keys.contains(&"PK"),
        fk: keys.contains(&"FK"),
        note,
    })
}

fn parse_link(line: &str) -> Option<Link> {
    let (rel, label) = line.split_once(':')?;
    let label = label.trim().trim_matches('"').to_string();
    let mut parts = rel.split_whitespace();
    let from = entity_name(parts.next()?);
    let marks = parts.next()?;
    let to = entity_name(parts.next()?);
    let sep = marks.find("--").or_else(|| marks.find(".."))?;
    let from_card = left_card(&marks[..sep])?;
    let to_card = right_card(&marks[sep + 2..])?;
    Some(Link {
        from,
        to,
        label,
        from_card,
        to_card,
    })
}

fn left_card(s: &str) -> Option<Card> {
    Some(match s {
        "||" => Card::ExactlyOne,
        "|o" => Card::ZeroOrOne,
        "}|" => Card::OneOrMore,
        "}o" => Card::ZeroOrMore,
        _ => return None,
    })
}

fn right_card(s: &str) -> Option<Card> {
    Some(match s {
        "||" => Card::ExactlyOne,
        "o|" => Card::ZeroOrOne,
        "|{" => Card::OneOrMore,
        "o{" => Card::ZeroOrMore,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Vec<Block> {
        blocks(include_str!("../tests/fixtures/library.md"))
    }

    fn table<'a>(d: &'a Diagram, name: &str) -> &'a Table {
        d.tables.iter().find(|t| t.name == name).unwrap()
    }

    fn column<'a>(t: &'a Table, name: &str) -> &'a Column {
        t.columns.iter().find(|c| c.name == name).unwrap()
    }

    #[test]
    fn columns_with_keys_and_notes() {
        let d = parse(
            "erDiagram\n  a {\n    text x PK, FK \"the note\"\n    text[] y\n  }\n  a ||--o{ b : \"lbl\"\n",
        );
        let a = table(&d, "a");
        assert_eq!(
            a.columns[0],
            Column {
                name: "x".into(),
                ty: "text".into(),
                pk: true,
                fk: true,
                note: Some("the note".into())
            }
        );
        assert_eq!(a.columns[1].ty, "text[]");
        assert!(!a.columns[1].pk);
        assert_eq!(
            d.links[0],
            Link {
                from: "a".into(),
                to: "b".into(),
                label: "lbl".into(),
                from_card: Card::ExactlyOne,
                to_card: Card::ZeroOrMore
            }
        );
        assert!(d.tables.iter().any(|t| t.name == "b"));
    }

    #[test]
    fn all_cardinalities_and_bare_labels() {
        let d = parse("erDiagram\n a |o..|{ b : owns\n c }|--o| d : x\n");
        assert_eq!(
            (d.links[0].from_card, d.links[0].to_card),
            (Card::ZeroOrOne, Card::OneOrMore)
        );
        assert_eq!(d.links[0].label, "owns");
        assert_eq!(
            (d.links[1].from_card, d.links[1].to_card),
            (Card::OneOrMore, Card::ZeroOrOne)
        );
    }

    #[test]
    fn blocks_take_the_nearest_heading() {
        let b = library();
        let titles: Vec<&str> = b.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, ["Current", "Planned"]);
        let twice = blocks("# A\n```mermaid\nerDiagram\n```\n```mermaid\nerDiagram\n```\n");
        let titles: Vec<&str> = twice.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, ["A (1)", "A (2)"]);
    }

    #[test]
    fn library_current() {
        let d = &library()[0].diagram;
        assert_eq!(d.tables.len(), 3);
        assert_eq!(d.links.len(), 2);
        let book = table(d, "book");
        assert_eq!(
            column(book, "borrower_id").note.as_deref(),
            Some("null when on the shelf")
        );
        assert_eq!(column(table(d, "member"), "phones").ty, "text[]");
        assert_eq!(d.links[1].label, "has borrowed");
        assert_eq!(d.links[1].from_card, Card::ZeroOrOne);
    }

    #[test]
    fn library_planned() {
        let d = &library()[1].diagram;
        assert_eq!(d.tables.len(), 11);
        assert_eq!(d.links.len(), 11);
        assert!(
            table(d, "book_author")
                .columns
                .iter()
                .take(2)
                .all(|c| c.pk && c.fk)
        );
        assert_eq!(table(d, "branch").columns.len(), 2);
        assert!(table(d, "card").columns.is_empty());
        assert!(table(d, "payment").columns.is_empty());
        let cards = |from: &str, to: &str| {
            let l = d
                .links
                .iter()
                .find(|l| l.from == from && l.to == to)
                .unwrap();
            (l.from_card, l.to_card)
        };
        assert_eq!(
            cards("author", "book_author"),
            (Card::ExactlyOne, Card::OneOrMore)
        );
        assert_eq!(cards("member", "card"), (Card::ExactlyOne, Card::ZeroOrOne));
        assert_eq!(cards("loan", "fine"), (Card::ZeroOrMore, Card::ExactlyOne));
        assert_eq!(cards("fine", "payment"), (Card::OneOrMore, Card::ZeroOrOne));
        assert!(d.links.iter().any(|l| l.label == "shelves"));
    }
}
