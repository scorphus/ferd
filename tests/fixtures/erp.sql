-- Whole company, Postgres.

create table currency (
    code text primary key,
    name text not null
);

create table country (
    code text primary key,
    currency_code text not null references currency(code),
    name text not null,
    description text
);

create table language (
    code text primary key,
    name text not null
);

-- One row per unit.
create table unit (
    code text primary key,
    name text not null,
    description text
);

create table time_zone (
    code text primary key,
    name text not null,
    updated_at timestamptz
);

create table unit_conversion (
    id bigint primary key,
    from_unit_code text not null references unit(code),
    to_unit_code text not null references unit(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table exchange_rate (
    id bigint primary key,
    from_currency_code text not null references currency(code),
    to_currency_code text not null references currency(code),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table company (
    id bigint primary key,
    currency_code text not null,
    country_code text not null,
    time_zone_code text not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (currency_code) references currency(code),
    foreign key (country_code) references country(code),
    foreign key (time_zone_code) references time_zone(code)
);

create table branch (
    id bigint primary key,
    company_id bigint not null references company(id),
    country_code text not null references country(code),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table region (
    id bigint primary key,
    country_code text not null references country(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table address (
    id bigint primary key,
    country_code text not null references country(code),
    region_id bigint references region(id), -- Null when there is no region.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table calendar (
    id bigint primary key,
    company_id bigint not null,
    name text,
    starts_on date not null,
    ends_on date,
    foreign key (company_id) references company(id)
);

create table holiday (
    id bigint primary key,
    calendar_id bigint not null references calendar(id),
    name text,
    starts_on date not null,
    ends_on date
);

create table fiscal_year (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text,
    starts_on date not null,
    ends_on date,
    updated_at timestamptz
);

-- One row per fiscal period.
create table fiscal_period (
    id bigint primary key,
    fiscal_year_id bigint not null references fiscal_year(id),
    name text,
    starts_on date not null,
    ends_on date
);

create table document_type (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table number_sequence (
    id bigint primary key,
    document_type_id bigint not null references document_type(id),
    company_id bigint not null references company(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table department (
    id bigint primary key,
    company_id bigint not null references company(id),
    parent_id bigint references department(id),
    manager_id bigint, -- Null when there is no manager. @ref: employee(id)
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column department.parent_id is 'Null at the top.';
comment on column department.status is 'Draft, open, closed or cancelled.';

create table job_title (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table job_position (
    id bigint primary key,
    department_id bigint not null,
    job_title_id bigint not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (department_id) references department(id),
    foreign key (job_title_id) references job_title(id)
);

create table employee (
    id bigint primary key,
    company_id bigint not null references company(id),
    department_id bigint not null references department(id),
    job_position_id bigint references job_position(id), -- Null when there is no job position.
    manager_id bigint references employee(id), -- Null when there is no manager.
    address_id bigint references address(id), -- Null when there is no address.
    name text not null,
    email text unique,
    phones text[],
    updated_at timestamptz
);

create table user_account (
    id bigint primary key,
    language_code text not null references language(code),
    time_zone_code text references time_zone(code), -- Null when there is no time zone.
    employee_id bigint references employee(id), -- Null when there is no employee.
    name text not null,
    email text unique,
    phones text[],
    updated_at timestamptz
);

create table attachment (
    id bigint primary key,
    uploaded_by_id bigint not null references user_account(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table note (
    id bigint primary key,
    user_account_id bigint not null,
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    foreign key (user_account_id) references user_account(id)
);

create table tag (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table tag_link (
    id bigint primary key,
    tag_id bigint not null references tag(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table setting (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table role (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table permission (
    id bigint primary key,
    code text unique,
    name text not null
);

-- One row per role permission.
create table role_permission (
    role_id bigint not null references role(id),
    permission_id bigint not null references permission(id),
    created_at timestamptz not null default now(),
    primary key (role_id, permission_id)
);

create table user_role (
    user_account_id bigint not null references user_account(id),
    role_id bigint not null references role(id),
    since date,
    primary key (user_account_id, role_id)
);

create table audit_log (
    id bigint primary key,
    user_account_id bigint not null,
    happened_at timestamptz not null,
    details jsonb,
    foreign key (user_account_id) references user_account(id)
);

create table employment_contract (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    job_position_id bigint not null references job_position(id),
    currency_code text not null references currency(code),
    number text unique,
    status text not null,
    issued_at timestamptz not null
);
comment on column employment_contract.status is 'Draft, open, closed or cancelled.';

create table salary_band (
    id bigint primary key,
    job_title_id bigint not null references job_title(id),
    currency_code text not null references currency(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table salary_component (
    id bigint primary key,
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table salary (
    id bigint primary key,
    employee_id bigint not null,
    salary_band_id bigint, -- Null when there is no salary band.
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (employee_id) references employee(id),
    foreign key (salary_band_id) references salary_band(id)
);

-- One row per payroll run.
create table payroll_run (
    id bigint primary key,
    company_id bigint not null references company(id),
    fiscal_period_id bigint not null references fiscal_period(id),
    happened_at timestamptz not null,
    details jsonb
);

create table payslip (
    id bigint primary key,
    payroll_run_id bigint not null references payroll_run(id),
    employee_id bigint not null references employee(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table payslip_line (
    id bigint primary key,
    payslip_id bigint not null references payslip(id),
    salary_component_id bigint not null references salary_component(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table timesheet (
    id bigint primary key,
    employee_id bigint not null,
    approver_id bigint, -- Null when there is no approver.
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (employee_id) references employee(id),
    foreign key (approver_id) references employee(id)
);

create table project_type (
    id bigint primary key,
    code text unique,
    name text not null
);

-- One row per customer group.
create table customer_group (
    id bigint primary key,
    code text unique,
    name text not null,
    updated_at timestamptz
);

create table payment_term (
    id bigint primary key,
    code text unique,
    name text not null
);

create table sales_rep (
    id bigint primary key,
    employee_id bigint not null,
    name text not null,
    email text unique,
    phones text[],
    joined_on date,
    foreign key (employee_id) references employee(id)
);

create table customer (
    id bigint primary key,
    customer_group_id bigint references customer_group(id), -- Null when there is no customer group.
    currency_code text not null references currency(code),
    payment_term_id bigint not null references payment_term(id),
    sales_rep_id bigint references sales_rep(id), -- Null when there is no sales rep.
    language_code text references language(code), -- Null when there is no language.
    name text not null,
    email text unique,
    phones text[]
);

create table contract (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    currency_code text not null references currency(code),
    sales_rep_id bigint references sales_rep(id), -- Null when there is no sales rep.
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric
);

create table project (
    id bigint primary key,
    project_type_id bigint not null references project_type(id),
    customer_id bigint references customer(id), -- Null when there is no customer.
    manager_id bigint not null references employee(id),
    company_id bigint not null references company(id),
    contract_id bigint references contract(id), -- Null when there is no contract.
    name text not null,
    created_at timestamptz not null default now()
);

create table project_phase (
    id bigint primary key,
    project_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (project_id) references project(id)
);

create table task (
    id bigint primary key,
    project_id bigint not null references project(id),
    project_phase_id bigint references project_phase(id), -- Null when there is no project phase.
    parent_id bigint references task(id), -- Null at the top.
    name text not null,
    created_at timestamptz not null default now()
);

create table timesheet_line (
    id bigint primary key,
    timesheet_id bigint not null references timesheet(id),
    project_id bigint references project(id), -- Null when there is no project.
    task_id bigint references task(id), -- Null when there is no task.
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric,
    updated_at timestamptz
);

create table leave_type (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table leave_request (
    id bigint primary key,
    employee_id bigint not null,
    leave_type_id bigint not null,
    approver_id bigint, -- Null when there is no approver.
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    foreign key (employee_id) references employee(id),
    foreign key (leave_type_id) references leave_type(id),
    foreign key (approver_id) references employee(id)
);

create table leave_balance (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    leave_type_id bigint not null references leave_type(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table training_course (
    id bigint primary key,
    code text unique,
    name text not null
);

create table training_session (
    id bigint primary key,
    training_course_id bigint not null references training_course(id),
    trainer_id bigint references employee(id), -- Null when there is no trainer.
    name text,
    starts_on date not null,
    ends_on date
);

create table training_enrollment (
    training_session_id bigint not null,
    employee_id bigint not null,
    since date,
    primary key (training_session_id, employee_id),
    foreign key (training_session_id) references training_session(id),
    foreign key (employee_id) references employee(id)
);

-- One row per skill.
create table skill (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table employee_skill (
    employee_id bigint not null references employee(id),
    skill_id bigint not null references skill(id),
    since date,
    primary key (employee_id, skill_id)
);

create table performance_review (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    reviewer_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz
);

create table review_goal (
    id bigint primary key,
    performance_review_id bigint not null,
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    foreign key (performance_review_id) references performance_review(id)
);

create table benefit_plan (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table benefit_enrollment (
    benefit_plan_id bigint not null references benefit_plan(id),
    employee_id bigint not null references employee(id),
    since date,
    primary key (benefit_plan_id, employee_id)
);

create table expense_claim (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    currency_code text not null references currency(code),
    approver_id bigint references employee(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null
);
comment on column expense_claim.approver_id is 'Null when there is no approver.';
comment on column expense_claim.status is 'Planned, running, done or failed.';

create table cost_center (
    id bigint primary key,
    company_id bigint not null,
    parent_id bigint, -- Null at the top.
    manager_id bigint, -- Null when there is no manager.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    foreign key (company_id) references company(id),
    foreign key (parent_id) references cost_center(id),
    foreign key (manager_id) references employee(id)
);

create table chart_of_accounts (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table account_type (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table account (
    id bigint primary key,
    chart_of_accounts_id bigint not null references chart_of_accounts(id),
    account_type_id bigint not null references account_type(id),
    parent_id bigint references account(id), -- Null at the top.
    currency_code text references currency(code), -- Null when there is no currency.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table tax_code (
    id bigint primary key,
    country_code text,
    account_id bigint not null,
    name text not null,
    status text,
    created_at timestamptz not null default now(),
    foreign key (country_code) references country(code),
    foreign key (account_id) references account(id)
);
comment on column tax_code.country_code is 'Null when there is no country.';
comment on column tax_code.status is 'Draft, open, closed or cancelled.';

create table expense_line (
    id bigint primary key,
    expense_claim_id bigint not null references expense_claim(id),
    cost_center_id bigint references cost_center(id), -- Null when there is no cost center.
    tax_code_id bigint references tax_code(id), -- Null when there is no tax code.
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table job_opening (
    id bigint primary key,
    department_id bigint not null references department(id),
    job_position_id bigint not null references job_position(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table candidate (
    id bigint primary key,
    job_opening_id bigint not null references job_opening(id),
    name text not null,
    email text unique,
    phones text[]
);

create table interview (
    id bigint primary key,
    candidate_id bigint not null,
    interviewer_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (candidate_id) references candidate(id),
    foreign key (interviewer_id) references employee(id)
);

create table sales_channel (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table customer_contact (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    name text not null,
    email text unique,
    phones text[]
);

create table customer_address (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    address_id bigint not null references address(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table price_list (
    id bigint primary key,
    currency_code text not null,
    company_id bigint not null,
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    foreign key (currency_code) references currency(code),
    foreign key (company_id) references company(id)
);

-- One row per product category.
create table product_category (
    id bigint primary key,
    parent_id bigint references product_category(id), -- Null at the top.
    code text unique,
    name text not null,
    description text
);

create table brand (
    id bigint primary key,
    code text unique,
    name text not null
);

create table product (
    id bigint primary key,
    product_category_id bigint not null references product_category(id),
    unit_code text not null references unit(code),
    brand_id bigint references brand(id), -- Null when there is no brand.
    tax_code_id bigint not null references tax_code(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table price_list_item (
    id bigint primary key,
    price_list_id bigint not null,
    product_id bigint not null,
    unit_code text not null,
    line_no int not null,
    quantity numeric not null,
    amount numeric,
    foreign key (price_list_id) references price_list(id),
    foreign key (product_id) references product(id),
    foreign key (unit_code) references unit(code)
);

create table discount_rule (
    id bigint primary key,
    price_list_id bigint not null references price_list(id),
    customer_group_id bigint references customer_group(id), -- Null when there is no customer group.
    product_category_id bigint references product_category(id), -- Null when there is no product category.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table lead_source (
    id bigint primary key,
    code text unique,
    name text not null,
    description text,
    updated_at timestamptz
);

create table lead (
    id bigint primary key,
    lead_source_id bigint not null references lead_source(id),
    owner_id bigint references sales_rep(id),
    name text not null,
    email text unique,
    phones text[]
);
comment on column lead.owner_id is 'Null when there is no owner.';

-- One row per opportunity.
create table opportunity (
    id bigint primary key,
    customer_id bigint not null,
    lead_id bigint, -- Null when there is no lead.
    sales_rep_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (customer_id) references customer(id),
    foreign key (lead_id) references lead(id),
    foreign key (sales_rep_id) references sales_rep(id)
);

create table campaign (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table campaign_member (
    campaign_id bigint not null references campaign(id),
    lead_id bigint not null references lead(id),
    created_at timestamptz not null default now(),
    primary key (campaign_id, lead_id)
);

create table quotation (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    sales_rep_id bigint not null references sales_rep(id),
    currency_code text not null references currency(code),
    opportunity_id bigint references opportunity(id), -- Null when there is no opportunity.
    price_list_id bigint not null references price_list(id),
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric
);

create table quotation_line (
    id bigint primary key,
    quotation_id bigint not null,
    product_id bigint not null,
    unit_code text not null,
    tax_code_id bigint not null,
    line_no int not null,
    quantity numeric not null,
    amount numeric,
    foreign key (quotation_id) references quotation(id),
    foreign key (product_id) references product(id),
    foreign key (unit_code) references unit(code),
    foreign key (tax_code_id) references tax_code(id)
);

create table sales_order (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    sales_rep_id bigint references sales_rep(id), -- Null when there is no sales rep.
    currency_code text not null references currency(code),
    payment_term_id bigint not null references payment_term(id),
    quotation_id bigint references quotation(id), -- Null when there is no quotation.
    billing_address_id bigint not null references customer_address(id),
    shipping_address_id bigint not null references customer_address(id),
    sales_channel_id bigint not null references sales_channel(id),
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null
);

create table warehouse (
    id bigint primary key,
    company_id bigint not null references company(id),
    address_id bigint not null references address(id),
    manager_id bigint references employee(id), -- Null when there is no manager.
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table sales_order_line (
    id bigint primary key,
    sales_order_id bigint not null references sales_order(id),
    product_id bigint not null references product(id),
    unit_code text not null references unit(code),
    tax_code_id bigint not null references tax_code(id),
    warehouse_id bigint references warehouse(id), -- Null when there is no warehouse.
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table commission_plan (
    id bigint primary key,
    company_id bigint not null,
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz,
    foreign key (company_id) references company(id)
);

create table commission (
    id bigint primary key,
    sales_rep_id bigint not null references sales_rep(id),
    commission_plan_id bigint not null references commission_plan(id),
    sales_order_id bigint not null references sales_order(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null
);
comment on column commission.status is 'Planned, running, done or failed.';

create table sales_target (
    id bigint primary key,
    sales_rep_id bigint not null references sales_rep(id),
    fiscal_period_id bigint not null references fiscal_period(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table return_reason (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

-- One row per sales return.
create table sales_return (
    id bigint primary key,
    sales_order_id bigint not null,
    customer_id bigint not null,
    return_reason_id bigint not null,
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    foreign key (sales_order_id) references sales_order(id),
    foreign key (customer_id) references customer(id),
    foreign key (return_reason_id) references return_reason(id)
);

create table sales_return_line (
    id bigint primary key,
    sales_return_id bigint not null references sales_return(id),
    sales_order_line_id bigint not null references sales_order_line(id),
    product_id bigint not null references product(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table contract_line (
    id bigint primary key,
    contract_id bigint not null references contract(id),
    product_id bigint not null references product(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table subscription (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    product_id bigint not null references product(id),
    contract_id bigint references contract(id), -- Null when there is no contract.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table customer_invoice (
    id bigint primary key,
    customer_id bigint not null,
    sales_order_id bigint, -- Null when there is no sales order.
    currency_code text not null,
    payment_term_id bigint not null,
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric,
    foreign key (customer_id) references customer(id),
    foreign key (sales_order_id) references sales_order(id),
    foreign key (currency_code) references currency(code),
    foreign key (payment_term_id) references payment_term(id)
);

create table subscription_period (
    id bigint primary key,
    subscription_id bigint not null references subscription(id),
    customer_invoice_id bigint references customer_invoice(id), -- Null when there is no customer invoice.
    name text,
    starts_on date not null,
    ends_on date,
    updated_at timestamptz
);

create table purchase_category (
    id bigint primary key,
    code text unique,
    name text not null
);

create table buyer (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    name text not null,
    email text unique,
    phones text[],
    joined_on date
);

create table supplier (
    id bigint primary key,
    currency_code text not null,
    payment_term_id bigint not null,
    country_code text not null,
    buyer_id bigint, -- Null when there is no buyer.
    name text not null,
    email text unique,
    phones text[],
    foreign key (currency_code) references currency(code),
    foreign key (payment_term_id) references payment_term(id),
    foreign key (country_code) references country(code),
    foreign key (buyer_id) references buyer(id)
);

create table supplier_contact (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    name text not null,
    email text unique,
    phones text[]
);

create table supplier_address (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    address_id bigint not null references address(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

-- One row per supplier rating.
create table supplier_rating (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    rated_by_id bigint not null references employee(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table purchase_requisition (
    id bigint primary key,
    requested_by_id bigint not null,
    department_id bigint not null,
    cost_center_id bigint,
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    foreign key (requested_by_id) references employee(id),
    foreign key (department_id) references department(id),
    foreign key (cost_center_id) references cost_center(id)
);
comment on column purchase_requisition.cost_center_id is 'Null when there is no cost center.';
comment on column purchase_requisition.status is 'Draft, open, closed or cancelled.';

create table requisition_line (
    id bigint primary key,
    purchase_requisition_id bigint not null references purchase_requisition(id),
    product_id bigint not null references product(id),
    unit_code text not null references unit(code),
    purchase_category_id bigint references purchase_category(id), -- Null when there is no purchase category.
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table request_for_quote (
    id bigint primary key,
    purchase_requisition_id bigint references purchase_requisition(id), -- Null when there is no purchase requisition.
    buyer_id bigint not null references buyer(id),
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz
);

create table rfq_line (
    id bigint primary key,
    request_for_quote_id bigint not null references request_for_quote(id),
    requisition_line_id bigint references requisition_line(id), -- Null when there is no requisition line.
    product_id bigint not null references product(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table rfq_supplier (
    request_for_quote_id bigint not null,
    supplier_id bigint not null,
    since date,
    primary key (request_for_quote_id, supplier_id),
    foreign key (request_for_quote_id) references request_for_quote(id),
    foreign key (supplier_id) references supplier(id)
);

create table supplier_quote (
    id bigint primary key,
    request_for_quote_id bigint not null references request_for_quote(id),
    supplier_id bigint not null references supplier(id),
    currency_code text not null references currency(code),
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric
);
comment on column supplier_quote.status is 'Planned, running, done or failed.';

create table supplier_quote_line (
    id bigint primary key,
    supplier_quote_id bigint not null references supplier_quote(id),
    rfq_line_id bigint not null references rfq_line(id),
    unit_code text not null references unit(code),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table incoterm (
    code text primary key,
    name text not null,
    description text
);

create table purchase_order (
    id bigint primary key,
    supplier_id bigint not null,
    buyer_id bigint not null,
    currency_code text not null,
    payment_term_id bigint not null,
    supplier_quote_id bigint, -- Null when there is no supplier quote.
    deliver_to_id bigint not null,
    incoterm_code text, -- Null when there is no incoterm.
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    total numeric,
    foreign key (supplier_id) references supplier(id),
    foreign key (buyer_id) references buyer(id),
    foreign key (currency_code) references currency(code),
    foreign key (payment_term_id) references payment_term(id),
    foreign key (supplier_quote_id) references supplier_quote(id),
    foreign key (deliver_to_id) references warehouse(id),
    foreign key (incoterm_code) references incoterm(code)
);

create table purchase_order_line (
    id bigint primary key,
    purchase_order_id bigint not null references purchase_order(id),
    product_id bigint not null references product(id),
    unit_code text not null references unit(code),
    tax_code_id bigint not null references tax_code(id),
    requisition_line_id bigint references requisition_line(id), -- Null when there is no requisition line.
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table goods_receipt (
    id bigint primary key,
    purchase_order_id bigint not null references purchase_order(id),
    warehouse_id bigint not null references warehouse(id),
    received_by_id bigint not null references employee(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null
);
comment on column goods_receipt.status is 'Planned, running, done or failed.';

create table storage_zone (
    id bigint primary key,
    warehouse_id bigint not null references warehouse(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table bin (
    id bigint primary key,
    storage_zone_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (storage_zone_id) references storage_zone(id)
);

create table lot (
    id bigint primary key,
    product_id bigint not null references product(id),
    supplier_id bigint references supplier(id), -- Null when there is no supplier.
    name text not null,
    created_at timestamptz not null default now()
);

-- One row per goods receipt line.
create table goods_receipt_line (
    id bigint primary key,
    goods_receipt_id bigint not null references goods_receipt(id),
    purchase_order_line_id bigint not null references purchase_order_line(id),
    bin_id bigint references bin(id), -- Null when there is no bin.
    lot_id bigint references lot(id), -- Null when there is no lot.
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table purchase_invoice (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    purchase_order_id bigint references purchase_order(id),
    currency_code text not null references currency(code),
    payment_term_id bigint not null references payment_term(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz
);
comment on column purchase_invoice.purchase_order_id is 'Null when there is no purchase order.';
comment on column purchase_invoice.status is 'Pending, approved or rejected.';

create table purchase_invoice_line (
    id bigint primary key,
    purchase_invoice_id bigint not null,
    purchase_order_line_id bigint, -- Null when there is no purchase order line.
    account_id bigint not null,
    tax_code_id bigint not null,
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric,
    foreign key (purchase_invoice_id) references purchase_invoice(id),
    foreign key (purchase_order_line_id) references purchase_order_line(id),
    foreign key (account_id) references account(id),
    foreign key (tax_code_id) references tax_code(id)
);

create table purchase_return (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    goods_receipt_id bigint not null references goods_receipt(id),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric
);

create table purchase_return_line (
    id bigint primary key,
    purchase_return_id bigint not null references purchase_return(id),
    goods_receipt_line_id bigint not null references goods_receipt_line(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table supplier_price (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    product_id bigint not null references product(id),
    currency_code text not null references currency(code),
    unit_code text not null references unit(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table blanket_order (
    id bigint primary key,
    supplier_id bigint not null,
    buyer_id bigint not null,
    currency_code text not null,
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric,
    foreign key (supplier_id) references supplier(id),
    foreign key (buyer_id) references buyer(id),
    foreign key (currency_code) references currency(code)
);
comment on column blanket_order.status is 'Draft, open, closed or cancelled.';

create table blanket_order_line (
    id bigint primary key,
    blanket_order_id bigint not null references blanket_order(id),
    product_id bigint not null references product(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table approval_rule (
    id bigint primary key,
    document_type_id bigint not null references document_type(id),
    company_id bigint not null references company(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table approval_step (
    id bigint primary key,
    approval_rule_id bigint not null references approval_rule(id),
    role_id bigint not null references role(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table approval (
    id bigint primary key,
    approval_step_id bigint not null,
    approver_id bigint not null,
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    updated_at timestamptz,
    foreign key (approval_step_id) references approval_step(id),
    foreign key (approver_id) references user_account(id)
);

create table product_variant (
    id bigint primary key,
    product_id bigint not null references product(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column product_variant.status is 'Draft, open, closed or cancelled.';

create table attribute (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table attribute_value (
    id bigint primary key,
    attribute_id bigint not null references attribute(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table variant_attribute (
    product_variant_id bigint not null,
    attribute_value_id bigint not null,
    created_at timestamptz not null default now(),
    primary key (product_variant_id, attribute_value_id),
    foreign key (product_variant_id) references product_variant(id),
    foreign key (attribute_value_id) references attribute_value(id)
);

create table barcode (
    id bigint primary key,
    product_variant_id bigint not null references product_variant(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table stock_level (
    product_variant_id bigint not null references product_variant(id),
    bin_id bigint not null references bin(id),
    on_hand int not null,
    reserved int,
    primary key (product_variant_id, bin_id)
);
comment on table stock_level is 'One row per stock level.';
comment on column stock_level.reserved is 'Held for orders not yet shipped.';

create table serial_number (
    id bigint primary key,
    product_variant_id bigint not null references product_variant(id),
    lot_id bigint references lot(id), -- Null when there is no lot.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table stock_move (
    id bigint primary key,
    product_variant_id bigint not null,
    from_bin_id bigint, -- Null when there is no from bin.
    to_bin_id bigint, -- Null when there is no to bin.
    unit_code text not null,
    lot_id bigint, -- Null when there is no lot.
    happened_at timestamptz not null,
    details jsonb,
    foreign key (product_variant_id) references product_variant(id),
    foreign key (from_bin_id) references bin(id),
    foreign key (to_bin_id) references bin(id),
    foreign key (unit_code) references unit(code),
    foreign key (lot_id) references lot(id)
);

create table stock_count (
    id bigint primary key,
    warehouse_id bigint not null references warehouse(id),
    counted_by_id bigint not null references employee(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table stock_count_line (
    id bigint primary key,
    stock_count_id bigint not null references stock_count(id),
    bin_id bigint not null references bin(id),
    product_variant_id bigint not null references product_variant(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table reorder_rule (
    id bigint primary key,
    product_variant_id bigint not null references product_variant(id),
    warehouse_id bigint not null references warehouse(id),
    preferred_id bigint references supplier(id),
    name text not null,
    status text,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);
comment on column reorder_rule.preferred_id is 'Null when there is no preferred.';
comment on column reorder_rule.status is 'New, in progress or resolved.';

-- One row per carrier.
create table carrier (
    id bigint primary key,
    code text unique,
    name text not null
);

create table transfer_order (
    id bigint primary key,
    from_warehouse_id bigint not null references warehouse(id),
    to_warehouse_id bigint not null references warehouse(id),
    carrier_id bigint references carrier(id), -- Null when there is no carrier.
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null
);

create table transfer_line (
    id bigint primary key,
    transfer_order_id bigint not null references transfer_order(id),
    product_variant_id bigint not null references product_variant(id),
    unit_code text not null references unit(code),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table stock_reservation (
    id bigint primary key,
    product_variant_id bigint not null references product_variant(id),
    sales_order_line_id bigint not null references sales_order_line(id),
    bin_id bigint not null references bin(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table adjustment_reason (
    id bigint primary key,
    code text unique,
    name text not null
);

-- One row per stock adjustment.
create table stock_adjustment (
    id bigint primary key,
    warehouse_id bigint not null references warehouse(id),
    adjustment_reason_id bigint not null references adjustment_reason(id),
    approved_by_id bigint references employee(id), -- Null when there is no approved by.
    name text not null,
    created_at timestamptz not null default now()
);

create table adjustment_line (
    id bigint primary key,
    stock_adjustment_id bigint not null references stock_adjustment(id),
    product_variant_id bigint not null references product_variant(id),
    bin_id bigint not null references bin(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table packaging (
    id bigint primary key,
    product_id bigint not null references product(id),
    unit_code text not null references unit(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table product_supplier (
    product_id bigint not null,
    supplier_id bigint not null,
    since date,
    primary key (product_id, supplier_id),
    foreign key (product_id) references product(id),
    foreign key (supplier_id) references supplier(id)
);

create table kit (
    id bigint primary key,
    product_id bigint not null references product(id),
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table kit_component (
    id bigint primary key,
    kit_id bigint not null references kit(id),
    product_variant_id bigint not null references product_variant(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table bill_of_materials (
    id bigint primary key,
    product_id bigint not null references product(id),
    unit_code text not null references unit(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table bom_line (
    id bigint primary key,
    bill_of_materials_id bigint not null,
    component_id bigint not null,
    unit_code text not null,
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric,
    foreign key (bill_of_materials_id) references bill_of_materials(id),
    foreign key (component_id) references product(id),
    foreign key (unit_code) references unit(code)
);

create table work_center (
    id bigint primary key,
    warehouse_id bigint references warehouse(id), -- Null when there is no warehouse.
    cost_center_id bigint not null references cost_center(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table shift (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text,
    starts_on date not null,
    ends_on date
);

create table work_center_capacity (
    id bigint primary key,
    work_center_id bigint not null references work_center(id),
    shift_id bigint not null references shift(id),
    name text not null,
    created_at timestamptz not null default now()
);

-- One row per routing.
create table routing (
    id bigint primary key,
    product_id bigint not null,
    bill_of_materials_id bigint, -- Null when there is no bill of materials.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (product_id) references product(id),
    foreign key (bill_of_materials_id) references bill_of_materials(id)
);

create table routing_step (
    id bigint primary key,
    routing_id bigint not null references routing(id),
    work_center_id bigint not null references work_center(id),
    name text not null,
    created_at timestamptz not null default now()
);

-- One row per depreciation method.
create table depreciation_method (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table asset_class (
    id bigint primary key,
    depreciation_method_id bigint not null references depreciation_method(id),
    code text unique,
    name text not null
);

create table asset_location (
    id bigint primary key,
    address_id bigint, -- Null when there is no address.
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (address_id) references address(id)
);

create table fixed_asset (
    id bigint primary key,
    asset_class_id bigint not null references asset_class(id),
    company_id bigint not null references company(id),
    asset_location_id bigint references asset_location(id), -- Null when there is no asset location.
    cost_center_id bigint references cost_center(id), -- Null when there is no cost center.
    supplier_id bigint references supplier(id), -- Null when there is no supplier.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table machine (
    id bigint primary key,
    work_center_id bigint not null references work_center(id),
    fixed_asset_id bigint references fixed_asset(id), -- Null when there is no fixed asset.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table tool (
    id bigint primary key,
    work_center_id bigint references work_center(id), -- Null when there is no work center.
    name text not null,
    created_at timestamptz not null default now()
);

create table production_order (
    id bigint primary key,
    product_id bigint not null,
    bill_of_materials_id bigint not null,
    routing_id bigint not null,
    warehouse_id bigint not null,
    sales_order_line_id bigint,
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric,
    foreign key (product_id) references product(id),
    foreign key (bill_of_materials_id) references bill_of_materials(id),
    foreign key (routing_id) references routing(id),
    foreign key (warehouse_id) references warehouse(id),
    foreign key (sales_order_line_id) references sales_order_line(id)
);
comment on column production_order.sales_order_line_id is 'Null when there is no sales order line.';
comment on column production_order.status is 'New, in progress or resolved.';

create table production_step (
    id bigint primary key,
    production_order_id bigint not null references production_order(id),
    routing_step_id bigint not null references routing_step(id),
    machine_id bigint references machine(id), -- Null when there is no machine.
    name text not null,
    created_at timestamptz not null default now()
);

create table material_issue (
    id bigint primary key,
    production_order_id bigint not null references production_order(id),
    product_variant_id bigint not null references product_variant(id),
    bin_id bigint not null references bin(id),
    lot_id bigint references lot(id), -- Null when there is no lot.
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null
);

create table production_output (
    id bigint primary key,
    production_order_id bigint not null references production_order(id),
    product_variant_id bigint not null references product_variant(id),
    bin_id bigint not null references bin(id),
    lot_id bigint references lot(id), -- Null when there is no lot.
    name text not null,
    created_at timestamptz not null default now()
);

create table scrap_reason (
    id bigint primary key,
    code text unique,
    name text not null
);

create table scrap (
    id bigint primary key,
    production_order_id bigint not null references production_order(id),
    product_variant_id bigint not null references product_variant(id),
    scrap_reason_id bigint not null references scrap_reason(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table shift_assignment (
    shift_id bigint not null references shift(id),
    employee_id bigint not null references employee(id),
    since date,
    primary key (shift_id, employee_id)
);

-- One row per downtime reason.
create table downtime_reason (
    id bigint primary key,
    code text unique,
    name text not null,
    description text,
    updated_at timestamptz
);

create table downtime (
    id bigint primary key,
    machine_id bigint not null,
    downtime_reason_id bigint not null,
    reported_by_id bigint not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (machine_id) references machine(id),
    foreign key (downtime_reason_id) references downtime_reason(id),
    foreign key (reported_by_id) references employee(id)
);

create table maintenance_type (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table maintenance_plan (
    id bigint primary key,
    machine_id bigint not null references machine(id),
    maintenance_type_id bigint not null references maintenance_type(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column maintenance_plan.status is 'Draft, open, closed or cancelled.';

create table technician (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    name text not null,
    email text unique,
    phones text[],
    joined_on date
);

create table maintenance_order (
    id bigint primary key,
    machine_id bigint not null,
    maintenance_plan_id bigint, -- Null when there is no maintenance plan.
    technician_id bigint, -- Null when there is no technician.
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    foreign key (machine_id) references machine(id),
    foreign key (maintenance_plan_id) references maintenance_plan(id),
    foreign key (technician_id) references technician(id)
);

create table spare_part (
    id bigint primary key,
    product_id bigint not null references product(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table maintenance_task (
    id bigint primary key,
    maintenance_order_id bigint not null references maintenance_order(id),
    spare_part_id bigint references spare_part(id), -- Null when there is no spare part.
    name text not null,
    created_at timestamptz not null default now()
);

create table mrp_run (
    id bigint primary key,
    company_id bigint not null references company(id),
    warehouse_id bigint not null references warehouse(id),
    happened_at timestamptz not null,
    details jsonb
);

create table planned_order (
    id bigint primary key,
    mrp_run_id bigint not null,
    product_id bigint not null,
    supplier_id bigint, -- Null when there is no supplier.
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric,
    foreign key (mrp_run_id) references mrp_run(id),
    foreign key (product_id) references product(id),
    foreign key (supplier_id) references supplier(id)
);

create table demand_forecast (
    id bigint primary key,
    company_id bigint not null references company(id),
    fiscal_period_id bigint not null references fiscal_period(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table forecast_line (
    id bigint primary key,
    demand_forecast_id bigint not null references demand_forecast(id),
    product_id bigint not null references product(id),
    warehouse_id bigint not null references warehouse(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table carrier_service (
    id bigint primary key,
    carrier_id bigint not null references carrier(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table shipping_zone (
    id bigint primary key,
    code text unique,
    name text not null
);

create table zone_country (
    shipping_zone_id bigint not null references shipping_zone(id),
    country_code text not null references country(code),
    created_at timestamptz not null default now(),
    primary key (shipping_zone_id, country_code)
);

create table shipping_rate (
    id bigint primary key,
    carrier_service_id bigint not null references carrier_service(id),
    shipping_zone_id bigint not null references shipping_zone(id),
    currency_code text not null references currency(code),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table vehicle (
    id bigint primary key,
    company_id bigint not null references company(id),
    fixed_asset_id bigint references fixed_asset(id), -- Null when there is no fixed asset.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table driver (
    id bigint primary key,
    employee_id bigint not null,
    name text not null,
    email text unique,
    phones text[],
    updated_at timestamptz,
    foreign key (employee_id) references employee(id)
);

create table delivery_route (
    id bigint primary key,
    warehouse_id bigint not null references warehouse(id),
    vehicle_id bigint not null references vehicle(id),
    driver_id bigint not null references driver(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column delivery_route.status is 'Draft, open, closed or cancelled.';

create table shipment (
    id bigint primary key,
    sales_order_id bigint references sales_order(id), -- Null when there is no sales order.
    warehouse_id bigint not null references warehouse(id),
    carrier_service_id bigint not null references carrier_service(id),
    incoterm_code text references incoterm(code), -- Null when there is no incoterm.
    delivery_route_id bigint references delivery_route(id), -- Null when there is no delivery route.
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric
);

create table shipment_line (
    id bigint primary key,
    shipment_id bigint not null references shipment(id),
    sales_order_line_id bigint references sales_order_line(id), -- Null when there is no sales order line.
    product_variant_id bigint not null references product_variant(id),
    lot_id bigint references lot(id), -- Null when there is no lot.
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

-- One row per package.
create table package (
    id bigint primary key,
    shipment_id bigint not null,
    packaging_id bigint, -- Null when there is no packaging.
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (shipment_id) references shipment(id),
    foreign key (packaging_id) references packaging(id)
);

create table package_item (
    id bigint primary key,
    package_id bigint not null references package(id),
    shipment_line_id bigint not null references shipment_line(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table tracking_event (
    id bigint primary key,
    shipment_id bigint not null references shipment(id),
    happened_at timestamptz not null,
    details jsonb
);

create table route_stop (
    id bigint primary key,
    delivery_route_id bigint not null references delivery_route(id),
    address_id bigint not null references address(id),
    shipment_id bigint references shipment(id), -- Null when there is no shipment.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table delivery_slot (
    id bigint primary key,
    warehouse_id bigint not null,
    line_no int not null,
    quantity numeric not null,
    amount numeric,
    foreign key (warehouse_id) references warehouse(id)
);

create table pick_list (
    id bigint primary key,
    warehouse_id bigint not null references warehouse(id),
    picker_id bigint references employee(id), -- Null when there is no picker.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table pick_line (
    id bigint primary key,
    pick_list_id bigint not null references pick_list(id),
    sales_order_line_id bigint not null references sales_order_line(id),
    bin_id bigint not null references bin(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table pack_station (
    id bigint primary key,
    warehouse_id bigint not null references warehouse(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column pack_station.status is 'New, in progress or resolved.';

create table dock (
    id bigint primary key,
    warehouse_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (warehouse_id) references warehouse(id)
);

create table dock_appointment (
    id bigint primary key,
    dock_id bigint not null references dock(id),
    carrier_id bigint not null references carrier(id),
    purchase_order_id bigint references purchase_order(id), -- Null when there is no purchase order.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table customs_declaration (
    id bigint primary key,
    shipment_id bigint not null references shipment(id),
    country_code text not null references country(code),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric
);

-- One row per customs line.
create table customs_line (
    id bigint primary key,
    customs_declaration_id bigint not null references customs_declaration(id),
    shipment_line_id bigint not null references shipment_line(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table freight_invoice (
    id bigint primary key,
    carrier_id bigint not null,
    currency_code text not null,
    purchase_invoice_id bigint,
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    foreign key (carrier_id) references carrier(id),
    foreign key (currency_code) references currency(code),
    foreign key (purchase_invoice_id) references purchase_invoice(id)
);
comment on column freight_invoice.purchase_invoice_id is 'Null when there is no purchase invoice.';
comment on column freight_invoice.status is 'Pending, approved or rejected.';

create table freight_charge (
    id bigint primary key,
    freight_invoice_id bigint not null references freight_invoice(id),
    shipment_id bigint not null references shipment(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table return_label (
    id bigint primary key,
    shipment_id bigint not null references shipment(id),
    sales_return_id bigint not null references sales_return(id),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz
);

create table journal (
    id bigint primary key,
    company_id bigint not null references company(id),
    default_account_id bigint references account(id), -- Null when there is no default account.
    name text not null,
    created_at timestamptz not null default now()
);

create table journal_entry (
    id bigint primary key,
    journal_id bigint not null,
    fiscal_period_id bigint not null,
    posted_by_id bigint, -- Null when there is no posted by.
    happened_at timestamptz not null,
    details jsonb,
    foreign key (journal_id) references journal(id),
    foreign key (fiscal_period_id) references fiscal_period(id),
    foreign key (posted_by_id) references user_account(id)
);

create table profit_center (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table journal_line (
    id bigint primary key,
    journal_entry_id bigint not null references journal_entry(id),
    account_id bigint not null references account(id),
    cost_center_id bigint references cost_center(id), -- Null when there is no cost center.
    profit_center_id bigint references profit_center(id), -- Null when there is no profit center.
    currency_code text not null references currency(code),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

-- One row per tax rate.
create table tax_rate (
    id bigint primary key,
    tax_code_id bigint not null references tax_code(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table tax_return (
    id bigint primary key,
    company_id bigint not null,
    fiscal_period_id bigint not null,
    country_code text not null,
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz,
    foreign key (company_id) references company(id),
    foreign key (fiscal_period_id) references fiscal_period(id),
    foreign key (country_code) references country(code)
);

create table tax_return_line (
    id bigint primary key,
    tax_return_id bigint not null references tax_return(id),
    tax_code_id bigint not null references tax_code(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table budget (
    id bigint primary key,
    company_id bigint not null references company(id),
    fiscal_year_id bigint not null references fiscal_year(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column budget.status is 'New, in progress or resolved.';

create table budget_line (
    id bigint primary key,
    budget_id bigint not null references budget(id),
    account_id bigint not null references account(id),
    cost_center_id bigint references cost_center(id), -- Null when there is no cost center.
    fiscal_period_id bigint not null references fiscal_period(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table bank (
    id bigint primary key,
    code text unique,
    name text not null
);

create table bank_account (
    id bigint primary key,
    bank_id bigint not null references bank(id),
    company_id bigint not null references company(id),
    currency_code text not null references currency(code),
    account_id bigint not null references account(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table bank_statement (
    id bigint primary key,
    bank_account_id bigint not null references bank_account(id),
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric
);

create table payment_method (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table payment (
    id bigint primary key,
    payment_method_id bigint not null,
    bank_account_id bigint not null,
    currency_code text not null,
    customer_id bigint, -- Null when there is no customer.
    supplier_id bigint, -- Null when there is no supplier.
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    foreign key (payment_method_id) references payment_method(id),
    foreign key (bank_account_id) references bank_account(id),
    foreign key (currency_code) references currency(code),
    foreign key (customer_id) references customer(id),
    foreign key (supplier_id) references supplier(id)
);

create table bank_statement_line (
    id bigint primary key,
    bank_statement_id bigint not null references bank_statement(id),
    payment_id bigint references payment(id), -- Null when there is no payment.
    line_no int not null,
    quantity numeric not null,
    amount numeric,
    updated_at timestamptz
);

-- One row per reconciliation.
create table reconciliation (
    id bigint primary key,
    bank_statement_line_id bigint not null references bank_statement_line(id),
    journal_line_id bigint not null references journal_line(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table payment_allocation (
    id bigint primary key,
    payment_id bigint not null references payment(id),
    customer_invoice_id bigint references customer_invoice(id), -- Null when there is no customer invoice.
    purchase_invoice_id bigint references purchase_invoice(id), -- Null when there is no purchase invoice.
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric
);

create table customer_invoice_line (
    id bigint primary key,
    customer_invoice_id bigint not null,
    sales_order_line_id bigint,
    account_id bigint not null,
    tax_code_id bigint not null,
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric,
    foreign key (customer_invoice_id) references customer_invoice(id),
    foreign key (sales_order_line_id) references sales_order_line(id),
    foreign key (account_id) references account(id),
    foreign key (tax_code_id) references tax_code(id)
);
comment on column customer_invoice_line.sales_order_line_id is 'Null when there is no sales order line.';

create table credit_note (
    id bigint primary key,
    customer_invoice_id bigint not null references customer_invoice(id),
    customer_id bigint not null references customer(id),
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    total numeric
);

create table credit_note_line (
    id bigint primary key,
    credit_note_id bigint not null references credit_note(id),
    customer_invoice_line_id bigint references customer_invoice_line(id), -- Null when there is no customer invoice line.
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric,
    updated_at timestamptz
);

create table dunning_level (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table dunning_notice (
    id bigint primary key,
    customer_invoice_id bigint not null,
    dunning_level_id bigint not null,
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    foreign key (customer_invoice_id) references customer_invoice(id),
    foreign key (dunning_level_id) references dunning_level(id)
);

create table depreciation_schedule (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    depreciation_method_id bigint not null references depreciation_method(id),
    fiscal_year_id bigint not null references fiscal_year(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table depreciation_entry (
    id bigint primary key,
    depreciation_schedule_id bigint not null references depreciation_schedule(id),
    journal_entry_id bigint references journal_entry(id), -- Null when there is no journal entry.
    fiscal_period_id bigint not null references fiscal_period(id),
    happened_at timestamptz not null,
    details jsonb
);

create table asset_transfer (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    from_location_id bigint not null references asset_location(id),
    to_location_id bigint not null references asset_location(id),
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table asset_disposal (
    id bigint primary key,
    fixed_asset_id bigint not null,
    journal_entry_id bigint, -- Null when there is no journal entry.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (fixed_asset_id) references fixed_asset(id),
    foreign key (journal_entry_id) references journal_entry(id)
);

create table asset_revaluation (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    journal_entry_id bigint references journal_entry(id), -- Null when there is no journal entry.
    name text not null,
    created_at timestamptz not null default now()
);

create table lease (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    supplier_id bigint not null references supplier(id),
    currency_code text not null references currency(code),
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric
);
comment on column lease.status is 'Draft, open, closed or cancelled.';

create table lease_payment (
    id bigint primary key,
    lease_id bigint not null references lease(id),
    payment_id bigint references payment(id), -- Null when there is no payment.
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null
);

-- One row per insurance policy.
create table insurance_policy (
    id bigint primary key,
    supplier_id bigint not null,
    currency_code text not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (supplier_id) references supplier(id),
    foreign key (currency_code) references currency(code)
);

create table insured_asset (
    insurance_policy_id bigint not null references insurance_policy(id),
    fixed_asset_id bigint not null references fixed_asset(id),
    created_at timestamptz not null default now(),
    primary key (insurance_policy_id, fixed_asset_id)
);

create table insurance_claim (
    id bigint primary key,
    insurance_policy_id bigint not null references insurance_policy(id),
    fixed_asset_id bigint not null references fixed_asset(id),
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null
);

create table asset_inspection (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    inspector_id bigint not null references employee(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric
);
comment on column asset_inspection.status is 'Planned, running, done or failed.';

create table warranty (
    id bigint primary key,
    fixed_asset_id bigint not null,
    supplier_id bigint not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    updated_at timestamptz,
    foreign key (fixed_asset_id) references fixed_asset(id),
    foreign key (supplier_id) references supplier(id)
);

create table warranty_claim (
    id bigint primary key,
    warranty_id bigint not null references warranty(id),
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null
);

create table asset_document (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    attachment_id bigint not null references attachment(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table meter (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    unit_code text not null references unit(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table meter_reading (
    id bigint primary key,
    meter_id bigint not null,
    read_by_id bigint,
    happened_at timestamptz not null,
    details jsonb,
    foreign key (meter_id) references meter(id),
    foreign key (read_by_id) references employee(id)
);
comment on column meter_reading.read_by_id is 'Null when there is no read by.';

create table asset_component (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    product_id bigint references product(id), -- Null when there is no product.
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

create table asset_assignment (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    employee_id bigint not null references employee(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

-- One row per asset reservation.
create table asset_reservation (
    id bigint primary key,
    fixed_asset_id bigint not null references fixed_asset(id),
    employee_id bigint not null references employee(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table asset_audit (
    id bigint primary key,
    company_id bigint not null,
    auditor_id bigint not null,
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    foreign key (company_id) references company(id),
    foreign key (auditor_id) references employee(id)
);

create table asset_audit_line (
    id bigint primary key,
    asset_audit_id bigint not null references asset_audit(id),
    fixed_asset_id bigint not null references fixed_asset(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric,
    updated_at timestamptz
);

create table project_member (
    project_id bigint not null references project(id),
    employee_id bigint not null references employee(id),
    since date,
    primary key (project_id, employee_id)
);

create table milestone (
    id bigint primary key,
    project_phase_id bigint not null references project_phase(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table task_dependency (
    id bigint primary key,
    task_id bigint not null,
    depends_on_id bigint not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (task_id) references task(id),
    foreign key (depends_on_id) references task(id)
);

create table task_assignment (
    task_id bigint not null references task(id),
    employee_id bigint not null references employee(id),
    created_at timestamptz not null default now(),
    primary key (task_id, employee_id)
);

create table time_entry (
    id bigint primary key,
    employee_id bigint not null references employee(id),
    task_id bigint not null references task(id),
    timesheet_line_id bigint references timesheet_line(id),
    happened_at timestamptz not null,
    details jsonb,
    updated_at timestamptz
);
comment on table time_entry is 'One row per time entry.';
comment on column time_entry.timesheet_line_id is 'Null when there is no timesheet line.';

create table project_budget (
    id bigint primary key,
    project_id bigint not null references project(id),
    currency_code text not null references currency(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table project_cost (
    id bigint primary key,
    project_id bigint not null,
    account_id bigint not null,
    journal_line_id bigint, -- Null when there is no journal line.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (project_id) references project(id),
    foreign key (account_id) references account(id),
    foreign key (journal_line_id) references journal_line(id)
);

create table project_invoice (
    id bigint primary key,
    project_id bigint not null references project(id),
    customer_invoice_id bigint not null references customer_invoice(id),
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    total numeric
);

create table project_risk (
    id bigint primary key,
    project_id bigint not null references project(id),
    owner_id bigint references employee(id), -- Null when there is no owner.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table issue (
    id bigint primary key,
    project_id bigint not null references project(id),
    reported_by_id bigint not null references employee(id),
    assigned_to_id bigint references employee(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric
);
comment on column issue.assigned_to_id is 'Null when there is no assigned to.';
comment on column issue.status is 'Planned, running, done or failed.';

create table issue_comment (
    id bigint primary key,
    issue_id bigint not null,
    user_account_id bigint not null,
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric,
    foreign key (issue_id) references issue(id),
    foreign key (user_account_id) references user_account(id)
);

create table change_request (
    id bigint primary key,
    project_id bigint not null references project(id),
    requested_by_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null
);

create table deliverable (
    id bigint primary key,
    milestone_id bigint not null references milestone(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table resource_plan (
    id bigint primary key,
    project_id bigint not null references project(id),
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table resource_booking (
    id bigint primary key,
    resource_plan_id bigint not null,
    employee_id bigint not null,
    happened_at timestamptz not null,
    details jsonb,
    foreign key (resource_plan_id) references resource_plan(id),
    foreign key (employee_id) references employee(id)
);

-- One row per project document.
create table project_document (
    id bigint primary key,
    project_id bigint not null references project(id),
    attachment_id bigint not null references attachment(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table status_report (
    id bigint primary key,
    project_id bigint not null references project(id),
    author_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric
);

create table rate_card (
    id bigint primary key,
    currency_code text not null references currency(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table rate_card_line (
    id bigint primary key,
    rate_card_id bigint not null,
    job_title_id bigint not null,
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric,
    foreign key (rate_card_id) references rate_card(id),
    foreign key (job_title_id) references job_title(id)
);

create table project_expense (
    id bigint primary key,
    project_id bigint not null references project(id),
    expense_line_id bigint references expense_line(id),
    name text not null,
    created_at timestamptz not null default now()
);
comment on column project_expense.expense_line_id is 'Null when there is no expense line.';

create table baseline (
    id bigint primary key,
    project_id bigint not null references project(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table ticket_category (
    id bigint primary key,
    parent_id bigint references ticket_category(id), -- Null at the top.
    code text unique,
    name text not null,
    description text
);

create table ticket_priority (
    id bigint primary key,
    code text unique,
    name text not null,
    updated_at timestamptz
);

create table support_team (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table team_member (
    support_team_id bigint not null references support_team(id),
    employee_id bigint not null references employee(id),
    since date,
    primary key (support_team_id, employee_id)
);

create table service_item (
    id bigint primary key,
    product_id bigint not null references product(id),
    customer_id bigint not null references customer(id),
    serial_number_id bigint references serial_number(id), -- Null when there is no serial number.
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

-- One row per ticket.
create table ticket (
    id bigint primary key,
    customer_id bigint not null,
    ticket_category_id bigint not null,
    ticket_priority_id bigint not null,
    support_team_id bigint not null,
    assigned_to_id bigint, -- Null when there is no assigned to.
    service_item_id bigint, -- Null when there is no service item.
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    foreign key (customer_id) references customer(id),
    foreign key (ticket_category_id) references ticket_category(id),
    foreign key (ticket_priority_id) references ticket_priority(id),
    foreign key (support_team_id) references support_team(id),
    foreign key (assigned_to_id) references employee(id),
    foreign key (service_item_id) references service_item(id)
);

create table ticket_comment (
    id bigint primary key,
    ticket_id bigint not null references ticket(id),
    user_account_id bigint not null references user_account(id),
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    total numeric
);

create table ticket_attachment (
    ticket_id bigint not null references ticket(id),
    attachment_id bigint not null references attachment(id),
    since date,
    primary key (ticket_id, attachment_id)
);

create table sla_policy (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table sla_target (
    id bigint primary key,
    sla_policy_id bigint not null,
    ticket_priority_id bigint not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (sla_policy_id) references sla_policy(id),
    foreign key (ticket_priority_id) references ticket_priority(id)
);

create table service_contract (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    sla_policy_id bigint not null references sla_policy(id),
    contract_id bigint references contract(id), -- Null when there is no contract.
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz
);

create table service_contract_line (
    id bigint primary key,
    service_contract_id bigint not null references service_contract(id),
    service_item_id bigint not null references service_item(id),
    line_no int not null,
    quantity numeric not null,
    unit_price numeric,
    amount numeric
);

create table technician_skill (
    technician_id bigint not null references technician(id),
    skill_id bigint not null references skill(id),
    since date,
    primary key (technician_id, skill_id)
);

create table field_visit (
    id bigint primary key,
    ticket_id bigint not null,
    technician_id bigint not null,
    address_id bigint not null,
    happened_at timestamptz not null,
    details jsonb,
    foreign key (ticket_id) references ticket(id),
    foreign key (technician_id) references technician(id),
    foreign key (address_id) references address(id)
);

create table visit_report (
    id bigint primary key,
    field_visit_id bigint not null references field_visit(id),
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric
);

-- One row per spare part usage.
create table spare_part_usage (
    id bigint primary key,
    field_visit_id bigint not null references field_visit(id),
    spare_part_id bigint not null references spare_part(id),
    lot_id bigint references lot(id), -- Null when there is no lot.
    happened_at timestamptz not null,
    details jsonb
);

create table knowledge_article (
    id bigint primary key,
    author_id bigint not null references employee(id),
    ticket_category_id bigint not null references ticket_category(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table article_feedback (
    id bigint primary key,
    knowledge_article_id bigint not null,
    customer_id bigint, -- Null when there is no customer.
    happened_at timestamptz not null,
    details jsonb,
    foreign key (knowledge_article_id) references knowledge_article(id),
    foreign key (customer_id) references customer(id)
);

create table escalation (
    id bigint primary key,
    ticket_id bigint not null references ticket(id),
    escalated_to_id bigint not null references support_team(id),
    name text not null,
    status text,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);
comment on column escalation.status is 'Draft, open, closed or cancelled.';

create table survey (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table survey_question (
    id bigint primary key,
    survey_id bigint not null references survey(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table survey_answer (
    id bigint primary key,
    survey_question_id bigint not null,
    ticket_id bigint, -- Null when there is no ticket.
    customer_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (survey_question_id) references survey_question(id),
    foreign key (ticket_id) references ticket(id),
    foreign key (customer_id) references customer(id)
);

create table warranty_registration (
    id bigint primary key,
    service_item_id bigint not null references service_item(id),
    customer_id bigint not null references customer(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table callback_request (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    ticket_id bigint references ticket(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null,
    total numeric
);
comment on column callback_request.ticket_id is 'Null when there is no ticket.';
comment on column callback_request.status is 'Planned, running, done or failed.';

create table test_method (
    id bigint primary key,
    code text unique,
    name text not null
);

create table inspection_plan (
    id bigint primary key,
    product_id bigint not null,
    test_method_id bigint, -- Null when there is no test method.
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    foreign key (product_id) references product(id),
    foreign key (test_method_id) references test_method(id)
);

-- One row per inspection point.
create table inspection_point (
    id bigint primary key,
    inspection_plan_id bigint not null references inspection_plan(id),
    test_method_id bigint not null references test_method(id),
    unit_code text references unit(code), -- Null when there is no unit.
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric
);

create table inspection (
    id bigint primary key,
    inspection_plan_id bigint not null references inspection_plan(id),
    goods_receipt_id bigint references goods_receipt(id), -- Null when there is no goods receipt.
    production_order_id bigint references production_order(id), -- Null when there is no production order.
    inspector_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric,
    updated_at timestamptz
);

create table inspection_result (
    id bigint primary key,
    inspection_id bigint not null references inspection(id),
    inspection_point_id bigint not null references inspection_point(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null
);
comment on column inspection_result.status is 'New, in progress or resolved.';

create table defect_type (
    id bigint primary key,
    code text unique,
    name text not null,
    description text
);

create table defect (
    id bigint primary key,
    inspection_id bigint not null references inspection(id),
    defect_type_id bigint not null references defect_type(id),
    product_variant_id bigint not null references product_variant(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table nonconformance (
    id bigint primary key,
    defect_id bigint references defect(id), -- Null when there is no defect.
    supplier_id bigint references supplier(id), -- Null when there is no supplier.
    product_id bigint not null references product(id),
    reported_by_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null
);

create table corrective_action (
    id bigint primary key,
    nonconformance_id bigint not null references nonconformance(id),
    owner_id bigint not null references employee(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table preventive_action (
    id bigint primary key,
    nonconformance_id bigint not null,
    owner_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (nonconformance_id) references nonconformance(id),
    foreign key (owner_id) references employee(id)
);

create table quality_audit (
    id bigint primary key,
    company_id bigint not null references company(id),
    auditor_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null
);

create table audit_finding (
    id bigint primary key,
    quality_audit_id bigint not null references quality_audit(id),
    nonconformance_id bigint references nonconformance(id), -- Null when there is no nonconformance.
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric
);

create table certificate (
    id bigint primary key,
    company_id bigint not null references company(id),
    issued_by_id bigint references supplier(id), -- Null when there is no issued by.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

-- One row per certificate requirement.
create table certificate_requirement (
    id bigint primary key,
    certificate_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (certificate_id) references certificate(id)
);

create table measuring_device (
    id bigint primary key,
    fixed_asset_id bigint references fixed_asset(id),
    unit_code text not null references unit(code),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column measuring_device.fixed_asset_id is 'Null when there is no fixed asset.';
comment on column measuring_device.status is 'Draft, open, closed or cancelled.';

create table calibration (
    id bigint primary key,
    measuring_device_id bigint not null references measuring_device(id),
    supplier_id bigint references supplier(id), -- Null when there is no supplier.
    name text not null,
    created_at timestamptz not null default now()
);

create table calibration_record (
    id bigint primary key,
    calibration_id bigint not null references calibration(id),
    performed_by_id bigint not null references employee(id),
    happened_at timestamptz not null,
    details jsonb
);

create table control_chart (
    id bigint primary key,
    inspection_point_id bigint not null,
    work_center_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (inspection_point_id) references inspection_point(id),
    foreign key (work_center_id) references work_center(id)
);

create table control_sample (
    id bigint primary key,
    control_chart_id bigint not null references control_chart(id),
    production_order_id bigint references production_order(id), -- Null when there is no production order.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table supplier_audit (
    id bigint primary key,
    supplier_id bigint not null references supplier(id),
    auditor_id bigint not null references employee(id),
    number text unique,
    status text not null,
    issued_at timestamptz not null
);
comment on column supplier_audit.status is 'Planned, running, done or failed.';

create table root_cause (
    id bigint primary key,
    code text unique,
    name text not null
);

create table complaint (
    id bigint primary key,
    customer_id bigint not null,
    product_id bigint not null,
    sales_order_id bigint, -- Null when there is no sales order.
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz,
    foreign key (customer_id) references customer(id),
    foreign key (product_id) references product(id),
    foreign key (sales_order_id) references sales_order(id)
);

create table complaint_cause (
    complaint_id bigint not null references complaint(id),
    root_cause_id bigint not null references root_cause(id),
    since date,
    primary key (complaint_id, root_cause_id)
);

create table eight_d_report (
    id bigint primary key,
    complaint_id bigint not null references complaint(id),
    owner_id bigint not null references employee(id),
    number text unique,
    status text not null, -- Planned, running, done or failed.
    issued_at timestamptz not null,
    total numeric
);

create table quality_cost (
    id bigint primary key,
    nonconformance_id bigint not null references nonconformance(id),
    account_id bigint not null references account(id),
    currency_code text not null references currency(code),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on table quality_cost is 'One row per quality cost.';
comment on column quality_cost.status is 'New, in progress or resolved.';

create table segment (
    id bigint primary key,
    company_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (company_id) references company(id)
);

create table segment_member (
    segment_id bigint not null references segment(id),
    customer_id bigint not null references customer(id),
    since date,
    primary key (segment_id, customer_id)
);

create table newsletter (
    id bigint primary key,
    company_id bigint not null references company(id),
    language_code text not null references language(code),
    name text not null,
    created_at timestamptz not null default now()
);

create table newsletter_issue (
    id bigint primary key,
    newsletter_id bigint not null references newsletter(id),
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null
);

create table subscription_consent (
    id bigint primary key,
    customer_id bigint not null,
    newsletter_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (customer_id) references customer(id),
    foreign key (newsletter_id) references newsletter(id)
);

create table ad_campaign (
    id bigint primary key,
    company_id bigint not null references company(id),
    currency_code text not null references currency(code),
    owner_id bigint not null references employee(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table ad_group (
    id bigint primary key,
    ad_campaign_id bigint not null references ad_campaign(id),
    code text unique,
    name text not null,
    description text
);

create table landing_page (
    id bigint primary key,
    ad_campaign_id bigint references ad_campaign(id), -- Null when there is no ad campaign.
    language_code text not null references language(code),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table ad (
    id bigint primary key,
    ad_group_id bigint not null,
    landing_page_id bigint, -- Null when there is no landing page.
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (ad_group_id) references ad_group(id),
    foreign key (landing_page_id) references landing_page(id)
);

create table ad_spend (
    id bigint primary key,
    ad_id bigint not null references ad(id),
    currency_code text not null references currency(code),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column ad_spend.status is 'Draft, open, closed or cancelled.';

-- One row per web form.
create table web_form (
    id bigint primary key,
    landing_page_id bigint not null references landing_page(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table form_submission (
    id bigint primary key,
    web_form_id bigint not null references web_form(id),
    lead_id bigint references lead(id), -- Null when there is no lead.
    happened_at timestamptz not null,
    details jsonb
);

create table web_visit (
    id bigint primary key,
    landing_page_id bigint not null,
    customer_id bigint, -- Null when there is no customer.
    happened_at timestamptz not null,
    details jsonb,
    foreign key (landing_page_id) references landing_page(id),
    foreign key (customer_id) references customer(id)
);

create table promotion (
    id bigint primary key,
    company_id bigint not null references company(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table coupon (
    id bigint primary key,
    ad_campaign_id bigint references ad_campaign(id),
    promotion_id bigint references promotion(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column coupon.ad_campaign_id is 'Null when there is no ad campaign.';
comment on column coupon.promotion_id is 'Null when there is no promotion.';
comment on column coupon.status is 'Draft, open, closed or cancelled.';

create table coupon_redemption (
    id bigint primary key,
    coupon_id bigint not null references coupon(id),
    sales_order_id bigint not null references sales_order(id),
    happened_at timestamptz not null,
    details jsonb,
    updated_at timestamptz
);

create table loyalty_program (
    id bigint primary key,
    company_id bigint not null,
    currency_code text not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (company_id) references company(id),
    foreign key (currency_code) references currency(code)
);

create table loyalty_tier (
    id bigint primary key,
    loyalty_program_id bigint not null references loyalty_program(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table loyalty_account (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    loyalty_program_id bigint not null references loyalty_program(id),
    loyalty_tier_id bigint not null references loyalty_tier(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table store (
    id bigint primary key,
    company_id bigint not null references company(id),
    address_id bigint not null references address(id),
    warehouse_id bigint not null references warehouse(id),
    manager_id bigint references employee(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column store.manager_id is 'Null when there is no manager.';
comment on column store.status is 'Draft, open, closed or cancelled.';

create table cash_register (
    id bigint primary key,
    store_id bigint not null,
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now(),
    foreign key (store_id) references store(id)
);

create table register_session (
    id bigint primary key,
    cash_register_id bigint not null references cash_register(id),
    cashier_id bigint not null references employee(id),
    name text,
    starts_on date not null,
    ends_on date
);

create table receipt (
    id bigint primary key,
    register_session_id bigint not null references register_session(id),
    customer_id bigint references customer(id), -- Null when there is no customer.
    currency_code text not null references currency(code),
    number text unique,
    status text not null, -- New, in progress or resolved.
    issued_at timestamptz not null,
    total numeric
);

create table loyalty_transaction (
    id bigint primary key,
    loyalty_account_id bigint not null references loyalty_account(id),
    sales_order_id bigint references sales_order(id), -- Null when there is no sales order.
    receipt_id bigint references receipt(id), -- Null when there is no receipt.
    happened_at timestamptz not null,
    details jsonb
);

create table referral (
    id bigint primary key,
    referrer_id bigint not null,
    referred_id bigint,
    name text not null,
    status text,
    created_at timestamptz not null default now(),
    foreign key (referrer_id) references customer(id),
    foreign key (referred_id) references customer(id)
);
comment on column referral.referred_id is 'Null when there is no referred.';
comment on column referral.status is 'New, in progress or resolved.';

create table social_post (
    id bigint primary key,
    ad_campaign_id bigint references ad_campaign(id), -- Null when there is no ad campaign.
    author_id bigint not null references employee(id),
    name text not null,
    created_at timestamptz not null default now()
);

-- One row per marketing event.
create table marketing_event (
    id bigint primary key,
    company_id bigint not null references company(id),
    address_id bigint references address(id), -- Null when there is no address.
    happened_at timestamptz not null,
    details jsonb
);

create table event_registration (
    marketing_event_id bigint not null references marketing_event(id),
    customer_id bigint not null references customer(id),
    created_at timestamptz not null default now(),
    primary key (marketing_event_id, customer_id)
);

create table persona (
    id bigint primary key,
    code text unique,
    name text not null,
    updated_at timestamptz
);

create table content_asset (
    id bigint primary key,
    persona_id bigint references persona(id),
    attachment_id bigint not null references attachment(id),
    author_id bigint not null references employee(id),
    name text not null,
    created_at timestamptz not null default now()
);
comment on column content_asset.persona_id is 'Null when there is no persona.';

create table store_hours (
    id bigint primary key,
    store_id bigint not null references store(id),
    name text,
    starts_on date not null,
    ends_on date
);

create table cash_drawer (
    id bigint primary key,
    cash_register_id bigint not null references cash_register(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table cash_count (
    id bigint primary key,
    register_session_id bigint not null,
    counted_by_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (register_session_id) references register_session(id),
    foreign key (counted_by_id) references employee(id)
);

-- One row per promotion rule.
create table promotion_rule (
    id bigint primary key,
    promotion_id bigint not null references promotion(id),
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table promotion_product (
    promotion_id bigint not null references promotion(id),
    product_id bigint not null references product(id),
    since date,
    primary key (promotion_id, product_id)
);

create table gift_card (
    id bigint primary key,
    currency_code text not null references currency(code),
    sold_in_id bigint references store(id), -- Null when there is no sold in.
    name text not null,
    created_at timestamptz not null default now()
);

create table receipt_line (
    id bigint primary key,
    receipt_id bigint not null,
    product_variant_id bigint not null,
    promotion_id bigint, -- Null when there is no promotion.
    line_no int not null,
    quantity numeric not null,
    amount numeric,
    foreign key (receipt_id) references receipt(id),
    foreign key (product_variant_id) references product_variant(id),
    foreign key (promotion_id) references promotion(id)
);

create table receipt_payment (
    id bigint primary key,
    receipt_id bigint not null references receipt(id),
    payment_method_id bigint not null references payment_method(id),
    gift_card_id bigint references gift_card(id), -- Null when there is no gift card.
    number text unique,
    status text not null, -- Draft, open, closed or cancelled.
    issued_at timestamptz not null,
    total numeric
);

create table gift_card_transaction (
    id bigint primary key,
    gift_card_id bigint not null references gift_card(id),
    receipt_id bigint references receipt(id), -- Null when there is no receipt.
    happened_at timestamptz not null,
    details jsonb
);

create table store_stock (
    store_id bigint not null references store(id),
    product_variant_id bigint not null references product_variant(id),
    on_hand int not null,
    reserved int,
    primary key (store_id, product_variant_id)
);
comment on column store_stock.reserved is 'Held for orders not yet shipped.';

create table shelf (
    id bigint primary key,
    store_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (store_id) references store(id)
);

create table planogram (
    id bigint primary key,
    product_category_id bigint not null references product_category(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table planogram_slot (
    id bigint primary key,
    planogram_id bigint not null references planogram(id),
    product_variant_id bigint not null references product_variant(id),
    line_no int not null,
    quantity numeric not null,
    amount numeric
);

-- One row per shelf planogram.
create table shelf_planogram (
    shelf_id bigint not null references shelf(id),
    planogram_id bigint not null references planogram(id),
    since date,
    primary key (shelf_id, planogram_id)
);

create table store_staff (
    store_id bigint not null,
    employee_id bigint not null,
    created_at timestamptz not null default now(),
    primary key (store_id, employee_id),
    foreign key (store_id) references store(id),
    foreign key (employee_id) references employee(id)
);

create table shift_report (
    id bigint primary key,
    store_id bigint not null references store(id),
    register_session_id bigint not null references register_session(id),
    name text,
    starts_on date not null,
    ends_on date
);

create table customer_display (
    id bigint primary key,
    cash_register_id bigint not null references cash_register(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table layaway (
    id bigint primary key,
    customer_id bigint not null references customer(id),
    store_id bigint not null references store(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table layaway_payment (
    id bigint primary key,
    layaway_id bigint not null,
    receipt_payment_id bigint, -- Null when there is no receipt payment.
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    foreign key (layaway_id) references layaway(id),
    foreign key (receipt_payment_id) references receipt_payment(id)
);

create table price_tag (
    id bigint primary key,
    product_variant_id bigint not null references product_variant(id),
    store_id bigint not null references store(id),
    name text not null,
    status text,
    created_at timestamptz not null default now()
);
comment on column price_tag.status is 'Draft, open, closed or cancelled.';

create table store_transfer (
    id bigint primary key,
    from_store_id bigint not null references store(id),
    to_store_id bigint not null references store(id),
    transfer_order_id bigint references transfer_order(id), -- Null when there is no transfer order.
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);

create table workflow (
    id bigint primary key,
    document_type_id bigint not null references document_type(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table workflow_state (
    id bigint primary key,
    workflow_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (workflow_id) references workflow(id)
);

create table workflow_transition (
    id bigint primary key,
    workflow_id bigint not null references workflow(id),
    from_state_id bigint not null references workflow_state(id),
    to_state_id bigint not null references workflow_state(id),
    role_id bigint references role(id), -- Null when there is no role.
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table workflow_instance (
    id bigint primary key,
    workflow_id bigint not null references workflow(id),
    current_state_id bigint not null references workflow_state(id),
    name text not null,
    created_at timestamptz not null default now()
);
comment on table workflow_instance is 'One row per workflow instance.';

create table workflow_task (
    id bigint primary key,
    workflow_instance_id bigint not null references workflow_instance(id),
    assignee_id bigint references user_account(id), -- Null when there is no assignee.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table notification_template (
    id bigint primary key,
    language_code text not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (language_code) references language(code)
);

create table notification (
    id bigint primary key,
    notification_template_id bigint not null references notification_template(id),
    user_account_id bigint not null references user_account(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table email_message (
    id bigint primary key,
    notification_id bigint references notification(id), -- Null when there is no notification.
    sent_by_id bigint references user_account(id), -- Null when there is no sent by.
    name text not null,
    created_at timestamptz not null default now()
);

create table email_recipient (
    id bigint primary key,
    email_message_id bigint not null references email_message(id),
    name text not null,
    status text,
    created_at timestamptz not null default now(),
    updated_at timestamptz
);
comment on column email_recipient.status is 'New, in progress or resolved.';

create table report_definition (
    id bigint primary key,
    owner_id bigint not null,
    number text unique,
    status text not null, -- Pending, approved or rejected.
    issued_at timestamptz not null,
    foreign key (owner_id) references user_account(id)
);

create table report_run (
    id bigint primary key,
    report_definition_id bigint not null references report_definition(id),
    user_account_id bigint not null references user_account(id),
    happened_at timestamptz not null,
    details jsonb
);

create table dashboard (
    id bigint primary key,
    owner_id bigint not null references user_account(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table dashboard_widget (
    id bigint primary key,
    dashboard_id bigint not null references dashboard(id),
    report_definition_id bigint references report_definition(id), -- Null when there is no report definition.
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table api_client (
    id bigint primary key,
    company_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (company_id) references company(id)
);

-- One row per api token.
create table api_token (
    id bigint primary key,
    api_client_id bigint not null references api_client(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table webhook (
    id bigint primary key,
    api_client_id bigint not null references api_client(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table webhook_delivery (
    id bigint primary key,
    webhook_id bigint not null references webhook(id),
    happened_at timestamptz not null,
    details jsonb
);

create table import_job (
    id bigint primary key,
    user_account_id bigint not null,
    document_type_id bigint, -- Null when there is no document type.
    name text not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz,
    foreign key (user_account_id) references user_account(id),
    foreign key (document_type_id) references document_type(id)
);

create table import_row_error (
    id bigint primary key,
    import_job_id bigint not null references import_job(id),
    happened_at timestamptz not null,
    details jsonb
);

create table export_job (
    id bigint primary key,
    user_account_id bigint not null references user_account(id),
    report_definition_id bigint references report_definition(id), -- Null when there is no report definition.
    name text not null,
    created_at timestamptz not null default now()
);

create table scheduled_job (
    id bigint primary key,
    owner_id bigint not null references user_account(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

create table job_run (
    id bigint primary key,
    scheduled_job_id bigint not null,
    happened_at timestamptz not null,
    details jsonb,
    foreign key (scheduled_job_id) references scheduled_job(id)
);

create table integration_mapping (
    id bigint primary key,
    api_client_id bigint not null references api_client(id),
    name text not null,
    status text, -- Draft, open, closed or cancelled.
    created_at timestamptz not null default now()
);

create table external_reference (
    id bigint primary key,
    integration_mapping_id bigint not null references integration_mapping(id),
    name text not null,
    created_at timestamptz not null default now()
);

create table data_retention_rule (
    id bigint primary key,
    document_type_id bigint not null references document_type(id),
    name text not null,
    status text, -- New, in progress or resolved.
    created_at timestamptz not null default now()
);

-- One row per feature flag.
create table feature_flag (
    id bigint primary key,
    company_id bigint not null,
    name text not null,
    created_at timestamptz not null default now(),
    foreign key (company_id) references company(id)
);
