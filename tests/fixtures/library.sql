-- Lending library, as planned: several branches, many copies of a book, and loans kept as
-- history. The same schema as the "Planned" diagram in library.md.

create extension if not exists pg_trgm with schema public;

create table author (
    id int primary key,
    name text not null
);

-- One row per edition.
create table book (
    id int primary key,
    title text not null,
    isbn text unique -- New: one per edition.
);

create table book_author (
    book_id int not null references book(id),
    author_id int not null references author(id),
    position int, -- Order on the cover.
    primary key (book_id, author_id)
);

create table branch (
    id int primary key,
    name text not null
);

create table copy (
    id int primary key,
    book_id int not null references book(id),
    branch_id int not null references branch(id),
    shelf text
);

create table member (
    id int primary key,
    name text not null,
    phones text[]
);

create table loan (
    id int primary key,
    copy_id int not null references copy(id),
    member_id int not null, -- @ref: member(id)
    taken date not null,
    due date not null,
    returned date -- Null while out.
);

create table hold (
    book_id int not null,
    member_id int not null,
    since timestamptz not null default now(),
    primary key (book_id, member_id),
    foreign key (book_id) references book(id),
    foreign key (member_id) references member(id)
);

create table card (
    id int primary key,
    member_id int not null unique references member(id),
    number text not null
);

create table payment (
    id int primary key,
    amount numeric not null,
    paid_on date not null
);

create table fine (
    id int primary key,
    loan_id int not null unique references loan(id),
    payment_id int references payment(id),
    amount numeric not null
);

create index book_title on book using gin (title gin_trgm_ops);

comment on table member is 'People who may borrow books.';
comment on column member.phones is 'Any order; the first one is called first.';
comment on column card.number is 'Printed on the card.';
comment on column fine.payment_id is 'Null until paid.';
