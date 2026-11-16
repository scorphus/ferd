create extension if not exists pg_trgm with schema public;

create table author (
    id integer primary key,
    name text not null
);

-- Books on the shelves.
create table book (
    id integer primary key,
    title text not null,
    author_id integer references author(id), -- One author per book, for now.
    borrower_id integer, -- Null when on the shelf. @ref: member(id)
    due date
);

create table member (
    id integer primary key,
    name text not null,
    phones text[],
    joined_at timestamp with time zone not null default now()
);

create table book_author (
    book_id integer not null references book(id),
    author_id integer not null references author(id),
    position integer,
    primary key (book_id, author_id)
);

create index book_title on book using gin (title gin_trgm_ops);

comment on table member is 'People who may borrow books.';
comment on column member.phones is 'Any order; the first one is called first.';

create materialized view overdue as
    select b.id as book_id, m.name as member, current_date - b.due as days
    from book b join member m on m.id = b.borrower_id
    where b.due < current_date
    with no data;

comment on materialized view overdue is 'Refreshed every night.';
