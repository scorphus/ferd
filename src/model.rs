use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagram {
    pub tables: Vec<Table>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub name: String,
    pub note: Option<String>,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Column {
    pub name: String,
    pub ty: String,
    pub pk: bool,
    pub fk: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub label: String,
    pub from_card: Card,
    pub to_card: Card,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Card {
    ExactlyOne,
    ZeroOrOne,
    OneOrMore,
    ZeroOrMore,
}

impl Card {
    pub const ALL: [Card; 4] = [
        Card::ExactlyOne,
        Card::ZeroOrOne,
        Card::OneOrMore,
        Card::ZeroOrMore,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Card::ExactlyOne => "exactly one",
            Card::ZeroOrOne => "zero or one",
            Card::OneOrMore => "one or more",
            Card::ZeroOrMore => "zero or more",
        }
    }
}
