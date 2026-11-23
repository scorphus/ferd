-- Corner shop, Postgres.

-- Makers of the products we sell.
create table brand (
    id bigint primary key,
    name text not null,
    website text
);

-- A tree; products sit in the leaves.
create table category (
    id bigint primary key,
    parent_id bigint references category(id), -- Null for top-level categories.
    name text not null,
    slug text unique -- Used in the shop's URLs.
);

create table product (
    id bigint primary key,
    brand_id bigint not null references brand(id),
    category_id bigint not null references category(id),
    name text not null,
    slug text unique,
    description text,
    tags text[],
    active boolean not null,
    created_at timestamptz not null default now()
);
comment on column product.active is 'Hidden from the shop when false.';

create table product_image (
    id bigint primary key,
    product_id bigint not null,
    url text not null,
    position int, -- Order in the gallery.
    alt_text text,
    foreign key (product_id) references product(id)
);

-- What a customer puts in the cart: one size and colour of a product.
create table variant (
    id bigint primary key,
    product_id bigint not null references product(id),
    sku text unique,
    size text,
    color text,
    weight_g int -- For shipping costs.
);

create table attribute (
    id bigint primary key,
    name text not null,
    unit text -- Null for words like cotton.
);

create table variant_attribute (
    variant_id bigint not null references variant(id),
    attribute_id bigint not null references attribute(id),
    value text not null,
    primary key (variant_id, attribute_id)
);

create table currency (
    code text primary key,
    name text not null,
    symbol text,
    decimals int not null
);
comment on column currency.code is 'ISO 4217.';

-- One price per variant and currency at a time; old prices stay for the record.
create table price (
    id bigint primary key,
    variant_id bigint not null references variant(id),
    currency_code text not null references currency(code),
    amount numeric not null,
    valid_from date not null,
    valid_to date -- Null while current.
);

create table collection (
    id bigint primary key,
    name text not null,
    slug text unique,
    starts_at timestamptz,
    ends_at timestamptz
);

create table collection_product (
    collection_id bigint not null references collection(id),
    product_id bigint not null references product(id),
    position int,
    primary key (collection_id, product_id)
);

create table customer (
    id bigint primary key,
    email text unique,
    name text not null,
    phones text[],
    marketing_opt_in boolean not null,
    created_at timestamptz not null default now()
);

create table country (
    code text primary key,
    name text not null,
    vat_rate numeric not null,
    ships_to boolean not null
);
comment on column country.code is 'ISO 3166 alpha-2.';

create table address (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    line1 text not null,
    line2 text,
    city text not null,
    postcode text,
    country_code text not null references country(code)
);

create table customer_group (
    id bigint primary key,
    name text not null,
    discount_percent numeric
);

create table customer_group_member (
    customer_group_id bigint not null,
    customer_id bigint not null,
    since date,
    primary key (customer_group_id, customer_id),
    foreign key (customer_group_id) references customer_group(id),
    foreign key (customer_id) references customer(id)
);

create table wishlist (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    name text not null,
    is_public boolean not null
);

create table wishlist_item (
    wishlist_id bigint not null references wishlist(id),
    variant_id bigint not null references variant(id),
    added_at timestamptz not null,
    primary key (wishlist_id, variant_id)
);

create table cart (
    id bigint primary key,
    customer_id bigint references customer(id), -- Null for guests.
    session_token text unique,
    updated_at timestamptz not null
);

create table cart_item (
    cart_id bigint not null,
    variant_id bigint not null,
    quantity int not null,
    primary key (cart_id, variant_id),
    foreign key (cart_id) references cart(id),
    foreign key (variant_id) references variant(id)
);

-- People who work in the back office.
create table staff (
    id bigint primary key,
    name text not null,
    email text unique,
    role text -- Support, warehouse or admin.
);

-- What a customer bought, as it was when they paid.
create table "order" (
    id bigint primary key,
    number text unique, -- Printed on the invoice.
    customer_id bigint not null references customer(id),
    billing_address_id bigint not null references address(id),
    shipping_address_id bigint not null references address(id),
    currency_code text not null references currency(code),
    status text not null, -- Placed, paid, shipped, delivered or cancelled.
    placed_at timestamptz not null,
    total numeric not null
);

create table order_line (
    order_id bigint not null references "order"(id),
    line_no int,
    variant_id bigint not null references variant(id),
    quantity int not null,
    unit_price numeric not null,
    discount numeric,
    primary key (order_id, line_no)
);
comment on column order_line.unit_price is 'Price when ordered.';

create table order_status_change (
    id bigint primary key,
    order_id bigint not null,
    from_status text,
    to_status text not null,
    changed_at timestamptz not null,
    changed_by_id bigint, -- Null when the customer did it.
    foreign key (order_id) references "order"(id),
    foreign key (changed_by_id) references staff(id)
);

create table coupon (
    id bigint primary key,
    code text unique,
    percent_off numeric,
    amount_off numeric, -- Used when percent_off is null.
    starts_at timestamptz,
    ends_at timestamptz,
    max_uses int
);

create table coupon_use (
    coupon_id bigint not null references coupon(id),
    order_id bigint not null references "order"(id),
    used_at timestamptz not null,
    primary key (coupon_id, order_id)
);

create table return_request (
    id bigint primary key,
    order_id bigint not null references "order"(id),
    reason text not null,
    status text not null, -- Requested, approved, received or refused.
    requested_at timestamptz not null
);

create table return_line (
    id bigint primary key,
    return_request_id bigint not null,
    variant_id bigint not null,
    quantity int not null,
    condition text,
    foreign key (return_request_id) references return_request(id),
    foreign key (variant_id) references variant(id)
);
comment on column return_line.condition is 'As checked in the warehouse.';

create table payment_method (
    id bigint primary key,
    name text not null,
    provider text not null,
    active boolean not null
);

create table payment (
    id bigint primary key,
    order_id bigint not null references "order"(id),
    payment_method_id bigint not null references payment_method(id),
    currency_code text not null references currency(code),
    amount numeric not null,
    status text not null, -- Pending, captured, failed or refunded.
    provider_ref text, -- Id at the payment provider.
    paid_at timestamptz
);

create table refund (
    id bigint primary key,
    return_request_id bigint references return_request(id), -- Null for goodwill refunds.
    payment_id bigint not null references payment(id),
    amount numeric not null,
    refunded_at timestamptz not null
);

create table gift_card (
    id bigint primary key,
    code text unique,
    balance numeric not null,
    currency_code text not null,
    expires_on date,
    foreign key (currency_code) references currency(code)
);

create table gift_card_use (
    gift_card_id bigint not null references gift_card(id),
    payment_id bigint not null references payment(id),
    amount numeric not null,
    primary key (gift_card_id, payment_id)
);

create table invoice (
    id bigint primary key,
    order_id bigint not null references "order"(id),
    number text unique,
    issued_on date not null,
    pdf_url text
);

create table carrier (
    id bigint primary key,
    name text not null,
    tracking_url text -- With {code} where the tracking code goes.
);

create table shipping_method (
    id bigint primary key,
    carrier_id bigint not null,
    name text not null,
    price numeric not null,
    days_min int,
    days_max int,
    foreign key (carrier_id) references carrier(id)
);

create table shipping_zone (
    id bigint primary key,
    name text not null
);

create table zone_country (
    shipping_zone_id bigint not null references shipping_zone(id),
    country_code text not null references country(code),
    primary key (shipping_zone_id, country_code)
);

create table warehouse (
    id bigint primary key,
    name text not null,
    street text,
    country_code text not null references country(code)
);

create table shipment (
    id bigint primary key,
    order_id bigint not null,
    shipping_method_id bigint not null,
    warehouse_id bigint not null,
    tracking_code text,
    shipped_at timestamptz,
    delivered_at timestamptz, -- Null until the carrier confirms.
    foreign key (order_id) references "order"(id),
    foreign key (shipping_method_id) references shipping_method(id),
    foreign key (warehouse_id) references warehouse(id)
);

create table shipment_line (
    shipment_id bigint not null references shipment(id),
    variant_id bigint not null references variant(id),
    quantity int not null,
    primary key (shipment_id, variant_id)
);

create table tracking_event (
    id bigint primary key,
    shipment_id bigint not null references shipment(id),
    status text not null,
    location text,
    happened_at timestamptz not null
);

create table stock (
    warehouse_id bigint not null references warehouse(id),
    variant_id bigint not null references variant(id),
    on_hand int not null,
    reserved int not null,
    primary key (warehouse_id, variant_id)
);
comment on column stock.reserved is 'Held for orders not yet shipped.';

create table supplier (
    id bigint primary key,
    name text not null,
    email text,
    country_code text not null,
    foreign key (country_code) references country(code)
);

create table supply (
    supplier_id bigint not null references supplier(id),
    variant_id bigint not null references variant(id),
    cost numeric not null,
    lead_days int,
    primary key (supplier_id, variant_id)
);

create table restock (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    warehouse_id bigint not null references warehouse(id),
    ordered_at timestamptz not null,
    expected_on date
);

create table restock_line (
    restock_id bigint not null references restock(id),
    variant_id bigint not null references variant(id),
    quantity int not null,
    received int, -- Null until the delivery is counted.
    primary key (restock_id, variant_id)
);

create table review (
    id bigint primary key,
    product_id bigint not null,
    customer_id bigint not null,
    rating int not null,
    title text,
    body text,
    created_at timestamptz not null default now(),
    foreign key (product_id) references product(id),
    foreign key (customer_id) references customer(id)
);
comment on column review.rating is '1 to 5 stars.';

create table review_vote (
    review_id bigint not null references review(id),
    customer_id bigint not null references customer(id),
    helpful boolean not null,
    primary key (review_id, customer_id)
);

create table question (
    id bigint primary key,
    product_id bigint not null references product(id),
    customer_id bigint not null references customer(id),
    body text not null,
    asked_at timestamptz not null
);

create table answer (
    id bigint primary key,
    question_id bigint not null references question(id),
    staff_id bigint references staff(id), -- Null when another customer answered.
    customer_id bigint references customer(id), -- Null when staff answered.
    body text not null
);

create materialized view bestseller as
    select l.variant_id, sum(l.quantity) as sold, sum(l.quantity * l.unit_price) as revenue
    from order_line l
    join "order" o on o.id = l.order_id
    where o.placed_at > now() - interval '30 days'
    group by l.variant_id
    with no data;

comment on materialized view bestseller is 'What sold best in the last 30 days. Refreshed every night.';
