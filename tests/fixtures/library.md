# Lending library

A small town library that lends books to its members.

## Current

One branch, one copy per book, and a loan is a row on the book itself.

```mermaid
erDiagram
    %% what runs today
    author {
        int id PK
        text name
    }
    book {
        int id PK
        text title
        int author_id FK "one author per book, for now"
        int borrower_id FK "null when on the shelf"
        date due
    }
    member {
        int id PK
        text name
        text[] phones
    }
    author ||--o{ book : writes
    member |o--o{ book : "has borrowed"
```

## Planned

Several branches, many copies of a book, and loans kept as history.

```mermaid
erDiagram
    author {
        int id PK
        text name
    }
    book {
        int id PK
        text title
        text isbn "new: one per edition"
    }
    book_author {
        int book_id PK, FK
        int author_id PK, FK
        int position "order on the cover"
    }
    branch["Branch"] {
        int id PK
        text name
    }
    copy {
        int id PK
        int book_id FK
        int branch_id FK
        text shelf
    }
    member {
        int id PK
        text name
        text[] phones
    }
    loan {
        int id PK
        int copy_id FK
        int member_id FK
        date taken
        date due
        date returned "null while out"
    }
    hold {
        int book_id PK, FK
        int member_id PK, FK
        timestamptz since
    }
    card {
        int id PK
        int member_id FK
        text number "printed on the card"
    }
    fine {
        int id PK
        int loan_id FK
        int payment_id FK "null until paid"
        numeric amount
    }
    payment {
        int id PK
        numeric amount
        date paid_on
    }
    author ||--|{ book_author : "is credited in"
    book ||--|{ book_author : "credits"
    book ||--o{ copy : "has copies"
    branch ||--o{ copy : shelves
    copy ||--o{ loan : "lent as"
    member ||--o{ loan : borrows
    book ||..o{ hold : "waited for in"
    member ||..o{ hold : places
    member ||--o| card : carries
    loan ||..o| fine : "may owe"
    fine }|..o| payment : "paid by"
```
