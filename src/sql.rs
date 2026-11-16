use crate::model::*;
use sqlparser::ast::{
    ColumnOption, CommentObject, Expr, ObjectName, Query, SelectItem, SetExpr, Statement,
    TableConstraint,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer, Whitespace};

struct ColumnText {
    name: String,
    ty: String,
    note: Option<String>,
}

struct CommentOn {
    table: String,
    column: Option<String>,
    text: Option<String>,
}

pub fn parse(src: &str) -> Diagram {
    let dialect = PostgreSqlDialect {};
    let Ok(tokens) = Tokenizer::new(&dialect, src).tokenize() else {
        return Diagram::default();
    };
    let mut diagram = Diagram::default();
    let mut refs: Vec<(String, String, String)> = Vec::new();
    let mut comments: Vec<CommentOn> = Vec::new();

    for segment in split_statements(tokens) {
        let segment = strip_with_data(segment);
        let Ok(statement) = Parser::new(&dialect)
            .with_tokens(segment.clone())
            .parse_statement()
        else {
            continue;
        };
        match statement {
            Statement::CreateTable(create) => {
                let name = last_ident(&create.name);
                let texts = column_texts(&segment);
                let mut pks: Vec<String> = Vec::new();
                let mut table = Table {
                    name: name.clone(),
                    note: leading_note(&segment),
                    columns: Vec::new(),
                };
                for constraint in &create.constraints {
                    match constraint {
                        TableConstraint::PrimaryKey(pk) => {
                            for column in &pk.columns {
                                if let Expr::Identifier(ident) = &column.column.expr {
                                    pks.push(ident.value.clone());
                                }
                            }
                        }
                        TableConstraint::ForeignKey(fk) => {
                            for column in &fk.columns {
                                refs.push((
                                    last_ident(&fk.foreign_table),
                                    name.clone(),
                                    column.value.clone(),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                for def in &create.columns {
                    let mut column = Column {
                        name: def.name.value.clone(),
                        pk: pks.contains(&def.name.value),
                        ..Column::default()
                    };
                    for option in &def.options {
                        match &option.option {
                            ColumnOption::PrimaryKey(_) => column.pk = true,
                            ColumnOption::ForeignKey(fk) => refs.push((
                                last_ident(&fk.foreign_table),
                                name.clone(),
                                column.name.clone(),
                            )),
                            _ => {}
                        }
                    }
                    if let Some(text) = texts.iter().find(|t| t.name == column.name) {
                        column.ty = text.ty.clone();
                        column.note = text.note.clone();
                    }
                    table.columns.push(column);
                }
                diagram.tables.push(table);
            }
            Statement::CreateView(view) if view.materialized => {
                diagram.tables.push(Table {
                    name: last_ident(&view.name),
                    note: leading_note(&segment),
                    columns: output_columns(&view.query)
                        .into_iter()
                        .map(|name| Column {
                            name,
                            ..Column::default()
                        })
                        .collect(),
                });
            }
            Statement::Comment {
                object_type,
                object_name,
                comment,
                ..
            } => {
                let parts: Vec<String> = object_name
                    .0
                    .iter()
                    .filter_map(|p| p.as_ident())
                    .map(|i| i.value.clone())
                    .collect();
                match (object_type, parts.as_slice()) {
                    (CommentObject::Table | CommentObject::MaterializedView, [.., table]) => {
                        comments.push(CommentOn {
                            table: table.clone(),
                            column: None,
                            text: comment,
                        })
                    }
                    (CommentObject::Column, [.., table, column]) => comments.push(CommentOn {
                        table: table.clone(),
                        column: Some(column.clone()),
                        text: comment,
                    }),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    for comment in comments {
        let Some(table) = diagram.tables.iter_mut().find(|t| t.name == comment.table) else {
            continue;
        };
        match comment.column {
            None => table.note = comment.text,
            Some(column) => {
                if let Some(column) = table.columns.iter_mut().find(|c| c.name == column) {
                    column.note = comment.text;
                }
            }
        }
    }

    for table in &diagram.tables {
        for column in &table.columns {
            for from in column.note.iter().flat_map(|n| ref_tables(n)) {
                refs.push((from, table.name.clone(), column.name.clone()));
            }
        }
    }
    for (from, to, label) in refs {
        for table in diagram.tables.iter_mut().filter(|t| t.name == to) {
            for column in table.columns.iter_mut().filter(|c| c.name == label) {
                column.fk = true;
            }
        }
        let exists = diagram
            .links
            .iter()
            .any(|l| l.from == from && l.to == to && l.label == label);
        if !exists {
            diagram.links.push(Link {
                from,
                to,
                label,
                from_card: Card::ExactlyOne,
                to_card: Card::ZeroOrMore,
            });
        }
    }
    diagram
}

fn last_ident(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|p| p.as_ident())
        .map(|i| i.value.clone())
        .unwrap_or_default()
}

fn split_statements(tokens: Vec<Token>) -> Vec<Vec<Token>> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::SemiColon if depth == 0 => {
                segments.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(token);
    }
    segments.push(current);
    segments
}

fn strip_with_data(mut tokens: Vec<Token>) -> Vec<Token> {
    let words: Vec<(usize, Keyword)> = tokens
        .iter()
        .enumerate()
        .filter_map(|(i, t)| match t {
            Token::Word(w) => Some((i, w.keyword)),
            _ => None,
        })
        .collect();
    let last_is_word = |idx: usize| {
        tokens[idx + 1..]
            .iter()
            .all(|t| matches!(t, Token::Whitespace(_)))
    };
    let n = words.len();
    let cut = match words.as_slice() {
        [.., (i, Keyword::WITH), (_, Keyword::NO), (d, Keyword::DATA)] if last_is_word(*d) => {
            Some(*i)
        }
        [.., (i, Keyword::WITH), (d, Keyword::DATA)] if n >= 2 && last_is_word(*d) => Some(*i),
        _ => None,
    };
    if let Some(i) = cut {
        tokens.truncate(i);
    }
    tokens
}

fn comment_text(token: &Token) -> Option<String> {
    match token {
        Token::Whitespace(Whitespace::SingleLineComment { comment, .. }) => {
            Some(comment.trim().to_string())
        }
        Token::Whitespace(Whitespace::MultiLineComment(comment)) => {
            Some(comment.trim().to_string())
        }
        _ => None,
    }
}

fn leading_note(tokens: &[Token]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut after_newline = false;
    for token in tokens {
        match token {
            Token::Whitespace(Whitespace::Newline) => {
                if after_newline {
                    lines.clear();
                }
                after_newline = true;
            }
            Token::Whitespace(Whitespace::Space | Whitespace::Tab) => {}
            Token::Whitespace(_) => {
                if let Some(text) = comment_text(token) {
                    lines.push(text);
                }
                after_newline = false;
            }
            _ => break,
        }
    }
    let note = lines.join(" ");
    (!note.is_empty()).then_some(note)
}

fn column_texts(tokens: &[Token]) -> Vec<ColumnText> {
    const TABLE_ITEMS: [Keyword; 6] = [
        Keyword::CONSTRAINT,
        Keyword::PRIMARY,
        Keyword::FOREIGN,
        Keyword::UNIQUE,
        Keyword::CHECK,
        Keyword::EXCLUDE,
    ];
    const TYPE_END: [Keyword; 8] = [
        Keyword::PRIMARY,
        Keyword::NOT,
        Keyword::NULL,
        Keyword::DEFAULT,
        Keyword::REFERENCES,
        Keyword::UNIQUE,
        Keyword::CHECK,
        Keyword::COLLATE,
    ];
    let mut columns: Vec<ColumnText> = Vec::new();
    let mut current: Option<usize> = None;
    let mut depth = 0usize;
    let mut at_start = false;
    let mut in_type = false;
    let mut ty = String::new();

    let finish_type = |columns: &mut Vec<ColumnText>, current: Option<usize>, ty: &mut String| {
        if let (Some(i), false) = (current, ty.is_empty()) {
            columns[i].ty = ty.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        ty.clear();
    };

    for token in tokens {
        if let Some(text) = comment_text(token) {
            if let (Some(i), true) = (current, depth >= 1) {
                let note = columns[i].note.get_or_insert_with(String::new);
                if !note.is_empty() {
                    note.push(' ');
                }
                note.push_str(&text);
            }
            continue;
        }
        match token {
            Token::Whitespace(_) => {
                if in_type {
                    ty.push(' ');
                }
            }
            Token::LParen => {
                depth += 1;
                if depth == 1 {
                    at_start = true;
                } else if in_type {
                    ty.push('(');
                }
            }
            Token::RParen => {
                if depth > 1 && in_type {
                    ty.push(')');
                }
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Token::Comma if depth == 1 => {
                finish_type(&mut columns, current, &mut ty);
                at_start = true;
                in_type = false;
            }
            _ if depth == 1 && at_start => {
                at_start = false;
                match token {
                    Token::Word(w)
                        if w.quote_style.is_none() && TABLE_ITEMS.contains(&w.keyword) =>
                    {
                        current = None;
                    }
                    Token::Word(w) => {
                        columns.push(ColumnText {
                            name: w.value.clone(),
                            ty: String::new(),
                            note: None,
                        });
                        current = Some(columns.len() - 1);
                        in_type = true;
                    }
                    _ => current = None,
                }
            }
            _ if in_type => match token {
                Token::Word(w)
                    if depth == 1 && w.quote_style.is_none() && TYPE_END.contains(&w.keyword) =>
                {
                    finish_type(&mut columns, current, &mut ty);
                    in_type = false;
                }
                _ => ty.push_str(&token.to_string()),
            },
            _ => {}
        }
    }
    finish_type(&mut columns, current, &mut ty);
    for column in &mut columns {
        if column.note.as_deref() == Some("") {
            column.note = None;
        }
    }
    columns
}

fn output_columns(query: &Query) -> Vec<String> {
    fn from_body(body: &SetExpr) -> Vec<String> {
        match body {
            SetExpr::Select(select) => select
                .projection
                .iter()
                .filter_map(|item| match item {
                    SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.clone()),
                    SelectItem::UnnamedExpr(Expr::Identifier(ident)) => Some(ident.value.clone()),
                    SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) => {
                        parts.last().map(|i| i.value.clone())
                    }
                    SelectItem::UnnamedExpr(expr) => Some(expr.to_string()),
                    _ => None,
                })
                .collect(),
            SetExpr::Query(query) => from_body(&query.body),
            SetExpr::SetOperation { left, .. } => from_body(left),
            _ => Vec::new(),
        }
    }
    from_body(&query.body)
}

fn ref_tables(note: &str) -> Vec<String> {
    note.split("@ref:")
        .skip(1)
        .filter_map(|rest| {
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '"'))
                .collect();
            let name = name.rsplit('.').next().unwrap_or("").trim_matches('"');
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table<'a>(d: &'a Diagram, name: &str) -> &'a Table {
        d.tables.iter().find(|t| t.name == name).unwrap()
    }

    fn column<'a>(t: &'a Table, name: &str) -> &'a Column {
        t.columns.iter().find(|c| c.name == name).unwrap()
    }

    fn link(d: &Diagram, from: &str, to: &str, label: &str) -> bool {
        d.links
            .iter()
            .any(|l| l.from == from && l.to == to && l.label == label)
    }

    #[test]
    fn column_types_as_written() {
        let d = parse(
            "create table t (a text, b text[] not null, c timestamp with time zone default now(), d bigint);",
        );
        let t = table(&d, "t");
        let types: Vec<&str> = t.columns.iter().map(|c| c.ty.as_str()).collect();
        assert_eq!(
            types,
            ["text", "text[]", "timestamp with time zone", "bigint"]
        );
    }

    #[test]
    fn primary_keys() {
        let d = parse(
            "create table a (id text primary key, x text);\n\
             create table b (x text, y text, z text, primary key (x, y));",
        );
        assert!(column(table(&d, "a"), "id").pk);
        assert!(!column(table(&d, "a"), "x").pk);
        let b = table(&d, "b");
        assert!(column(b, "x").pk && column(b, "y").pk && !column(b, "z").pk);
    }

    #[test]
    fn foreign_keys_and_links() {
        let d = parse(
            "create table p (id text primary key);\n\
             create table c (id text primary key, p1 text references p(id), p2 text, foreign key (p2) references p(id));",
        );
        let c = table(&d, "c");
        assert!(column(c, "p1").fk && column(c, "p2").fk && !column(c, "id").fk);
        assert!(link(&d, "p", "c", "p1"));
        assert!(link(&d, "p", "c", "p2"));
        assert_eq!(d.links[0].from_card, Card::ExactlyOne);
        assert_eq!(d.links[0].to_card, Card::ZeroOrMore);
    }

    #[test]
    fn ref_in_column_comment_keeps_note() {
        let d = parse("create table c (p text -- Parent. @ref: p(id)\n);");
        let col = column(table(&d, "c"), "p");
        assert!(col.fk);
        assert_eq!(col.note.as_deref(), Some("Parent. @ref: p(id)"));
        assert!(link(&d, "p", "c", "p"));
    }

    #[test]
    fn links_are_not_repeated() {
        let d = parse("create table c (p text references p(id) -- @ref: p(id)\n);");
        assert_eq!(d.links.len(), 1);
    }

    #[test]
    fn notes_from_line_comments() {
        let d = parse(
            "-- unrelated\n\n-- First line.\n-- Second line.\ncreate table t\n(\n    a text primary key, -- note a\n    b text -- note b\n);",
        );
        let t = table(&d, "t");
        assert_eq!(t.note.as_deref(), Some("First line. Second line."));
        assert_eq!(column(t, "a").note.as_deref(), Some("note a"));
        assert_eq!(column(t, "b").note.as_deref(), Some("note b"));
    }

    #[test]
    fn comment_after_table_constraint_is_ignored() {
        let d = parse("create table t (a text, b text, primary key (a, b), -- skip\n c text);");
        let t = table(&d, "t");
        assert_eq!(column(t, "b").note, None);
        assert_eq!(column(t, "c").note, None);
    }

    #[test]
    fn comment_on_statements_override() {
        let d = parse(
            "-- old\ncreate table t (a text -- old a\n);\n\
             comment on table t is 'new';\n\
             comment on column t.a is 'new a';\n\
             create materialized view v as select a from t;\n\
             comment on materialized view v is 'view note';",
        );
        assert_eq!(table(&d, "t").note.as_deref(), Some("new"));
        assert_eq!(column(table(&d, "t"), "a").note.as_deref(), Some("new a"));
        assert_eq!(table(&d, "v").note.as_deref(), Some("view note"));
    }

    #[test]
    fn materialized_view_columns() {
        let d = parse(
            "create materialized view v as\n with e as (select 1 as x)\n select e.x, y as why, count(*) as n from e with no data;",
        );
        let v = table(&d, "v");
        let names: Vec<&str> = v.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "why", "n"]);
        assert!(v.columns.iter().all(|c| c.ty.is_empty()));
    }

    #[test]
    fn unparsable_and_other_statements_are_skipped() {
        let d = parse(
            "create extension if not exists pg_trgm with schema public;\n\
             this is not sql;\n\
             create table t (a text);\n\
             create index i on t using btree (a);",
        );
        assert_eq!(d.tables.len(), 1);
    }

    #[test]
    fn library_schema() {
        let d = parse(include_str!("../tests/fixtures/library.sql"));
        let names: Vec<&str> = d.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["author", "book", "member", "book_author", "overdue"]
        );
        assert_eq!(
            table(&d, "book").note.as_deref(),
            Some("Books on the shelves.")
        );
        let ba = table(&d, "book_author");
        assert!(column(ba, "book_id").pk && column(ba, "author_id").pk);
        assert!(!column(ba, "position").pk);
        let borrower = column(table(&d, "book"), "borrower_id");
        assert!(borrower.fk);
        assert!(borrower.note.as_deref().unwrap().starts_with("Null when"));
        for (from, to, label) in [
            ("author", "book", "author_id"),
            ("member", "book", "borrower_id"),
            ("book", "book_author", "book_id"),
            ("author", "book_author", "author_id"),
        ] {
            assert!(link(&d, from, to, label), "{from} {to} {label}");
        }
        let member = table(&d, "member");
        assert_eq!(member.note.as_deref(), Some("People who may borrow books."));
        assert_eq!(column(member, "phones").ty, "text[]");
        assert!(column(member, "phones").note.is_some());
        assert_eq!(column(member, "joined_at").ty, "timestamp with time zone");
        let overdue = table(&d, "overdue");
        let cols: Vec<&str> = overdue.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(cols, ["book_id", "member", "days"]);
        assert_eq!(overdue.note.as_deref(), Some("Refreshed every night."));
    }
}
