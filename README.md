# AutoModel — SQL-first Reverse ORM for Rust, Built for the greater DX and for the AI Era

## Why AutoModel

Database access in Rust typically falls into two camps: **ORMs** (Diesel, SeaORM) and **compile-time checked SQL** (sqlx). Both have trade-offs that become sharply worse when an AI assistant — or any automated tool — is working with your code, and humans are exposed to far more intense code reviewing cycle.

**AutoModel is different: you write plain SQL, and the tool generates real Rust source files**.

```
queries/users/get_user.sql  →  src/generated/users.rs  (checked into git)
```

1. **Human or AI can read everything.** Generated structs, function signatures, error enums, and type aliases — all corresponding to the actual database schema, including constraints exposed as structured Rust enums — are ordinary `.rs` files sitting in your repo. An LLM can inspect them, reason about types, and produce correct calling code on the first try — no PostgreSQL agent, no database connection, no special tooling required.
2. **Plain SQL stays plain SQL.** Your queries are `.sql` files with full syntax highlighting. There is no query builder to learn, no expression DSL. Any valid PostgreSQL query works — window functions, CTEs, recursive queries, lateral joins, subqueries, aggregations, `UNNEST` batch inserts, partitioned tables, domain types, composite types, conditional clauses — all features of SQL, with no restrictions.
3. **Build-time code generation, not compile-time magic.** `build.rs` connects to the database once, extracts types from prepared statements, and writes `.rs` files — the whole step takes seconds, not the minutes of a full application compile. After that, builds are fully offline. CI can verify that generated code is up-to-date without a live database.
4. **Diff-friendly and reviewable.** Because the generated code is committed, pull request reviewers (human or AI) see exactly what changed — a renamed field, a new column, a constraint added. Nothing is hidden inside macro expansion.
5. **Built-in query analytics.** During code generation, AutoModel runs `EXPLAIN` on every query. Every generated function includes the query plan in its doc comments, and a warnings file is committed to the repo flagging sequential scans (missing indexes) and multi-partition access on partitioned tables. Warnings are surfaced during build time and visible at review time — reviewers (human or AI) catch performance problems before they reach production. Analysis can be opted out per query.
6. **Feature-rich control over generated code.** Struct reuse and deduplication across queries. Diff-based conditional updates for the load → transform → save pattern. Custom struct naming for cleaner, domain-specific APIs. Automated `multiunzip` support combined with `UNNEST` for batch inserts. Strongly typed mappings for `json`/`jsonb` columns. Full support for composite types and whole-record column insertion and selection — and much more.
7. **Less code to write, review, and test.** The glue between SQL and Rust — structs, parameter binding, error enums, type conversions — is an entire class of code that no human needs to write, review, or maintain. It is machine-generated from your SQL and the database schema. Reviewers focus on the `.sql` file and the business logic that calls it. This directly translates to faster development cycles: adding a new query is a single `.sql` file, and you have a strongly typed Rust function to call on the next build.

The result: a workflow where SQL is the source of truth, types are real files, every tool in the ecosystem — IDE, AI, CI, code review — can see the full picture, and development moves faster because an entire layer of boilerplate is eliminated.

## Project Structure

This is a Cargo workspace with three main components:

- **`automodel-lib/`** - The core library for generating typed functions from SQL queries
- **`automodel-cli/`** - Command-line interface with advanced features  
- **`example-app/`** - An example application that demonstrates build-time code generation

## Quick Start

### 1. Add to your Cargo.toml

```toml
[dependencies]
automodel = "0.9"

[build-dependencies]  
automodel = "0.9"
tokio = { version = "1.0", features = ["rt"] }
```

### 2. Create a build.rs

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let defaults = automodel::DefaultsConfig {
        telemetry: automodel::DefaultsTelemetryConfig {
            level: automodel::TelemetryLevel::Debug,
            include_sql: true,
        },
        ensure_indexes: true,
        derives: automodel::DefaultsDerivesConfig {
            return_type: vec!["Clone".to_string()],
            parameters_type: vec!["Clone".to_string()],
            conditions_type: vec!["Clone".to_string()],
            error_type: vec!["Clone".to_string()],
        },
    };
    automodel::AutoModel::generate(
        || {
            if std::env::var("CI").is_err() {
                std::env::var("AUTOMODEL_DATABASE_URL").map_err(|_| {
                    "AUTOMODEL_DATABASE_URL environment variable must be set for code generation"
                        .to_string()
                })
            } else {
                Err(
                    "Detecting not up to date AutoModel generated code in CI environment"
                        .to_string(),
                )
            }
        },
        "queries",
        "src/generated",
        defaults,
    )
    .await
}
```

### 3. Write SQL queries

Create a `queries/` directory and add `.sql` files organized by module:

```
my-project/
├── queries/
│   └── users/
│       ├── get_user_by_id.sql
│       ├── create_user.sql
│       └── update_user_profile.sql
├── build.rs
└── src/
    └── main.rs
```

Each SQL file contains an optional metadata block followed by the query:

```sql
-- @automodel
--    description: Retrieve a user by their ID
--    expect: exactly_one
-- @end

SELECT id, name, email, created_at
FROM users
WHERE id = #{id}
```

A more advanced example with conditional updates and custom types:

```sql
-- @automodel
--    description: Update user profile with conditional name/email
--    expect: exactly_one
--    conditions_type: true
--    types:
--      profile: "crate::models::UserProfile"
-- @end

UPDATE users 
SET profile = #{profile}, updated_at = NOW() 
#[, name = #{name?}] 
#[, email = #{email?}] 
WHERE id = #{user_id} 
RETURNING id, name, email, profile, updated_at
```

File path determines the generated module and function name: `queries/{module}/{function}.sql`. Both must be valid Rust identifiers.

All metadata is optional. When omitted, sensible defaults are used. See [Configuration Options](#configuration-options) for the full reference.

### 4. Use the generated functions

```rust
mod generated;

use tokio_postgres::Client;

async fn example(client: &Client) -> Result<(), tokio_postgres::Error> {
    let user = generated::get_user_by_id(client, 1).await?;
    let new_id = generated::create_user(client, "John".to_string(), "john@example.com".to_string()).await?;
    Ok(())
}
```

### 5. CLI Usage (alternative to build.rs)

AutoModel also ships as a standalone CLI for use outside of `build.rs`:

```bash
# Generate code from queries directory
automodel generate -d postgresql://localhost/mydb -q queries/

# Generate with custom output file
automodel generate -d postgresql://localhost/mydb -q queries/ -o src/db_functions.rs

# Dry run (see generated code without writing files)
automodel generate -d postgresql://localhost/mydb -q queries/ --dry-run

# Help
automodel --help
```

## Configuration Options

AutoModel uses SQL files with embedded metadata to define queries and their configuration. Here's a comprehensive guide to all configuration options:

### SQL File Structure

Each `.sql` file in the `queries/{module}/` directory contains:
1. Optional metadata block (in YAML format within SQL comments)
2. The SQL query

```sql
-- @automodel
--    description: Query description
--    expect: exactly_one
--    # ... other configuration options
-- @end

SELECT * FROM users WHERE id = #{id}
```

### Default Configuration

Defaults are configured in `build.rs` when calling `AutoModel::generate()`:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let defaults = automodel::DefaultsConfig {
        telemetry: automodel::DefaultsTelemetryConfig {
            level: automodel::TelemetryLevel::Debug,
            include_sql: true,
        },
        ensure_indexes: true,
        derives: automodel::DefaultsDerivesConfig {
            return_type: vec!["Clone".to_string()],
            parameters_type: vec!["Clone".to_string()],
            conditions_type: vec!["Clone".to_string()],
            error_type: vec!["Clone".to_string()],
        },
        // Use itertools for multiunzip (default, supports up to 12 parameters)
        // Change to ManyUnzip for queries with more than 12 parameters in batch inserts
        multiunzip_crate: automodel::MultiunzipCrate::Itertools,
    };
    automodel::AutoModel::generate(
        || {
            if std::env::var("CI").is_err() {
                std::env::var("AUTOMODEL_DATABASE_URL").map_err(|_| {
                    "AUTOMODEL_DATABASE_URL environment variable must be set for code generation"
                        .to_string()
                })
            } else {
                Err(
                    "Detecting not up to date AutoModel generated code in CI environment"
                        .to_string(),
                )
            }
        },
        "queries",
        "src/generated",
        defaults,
    )
    .await
}
```

**Telemetry Levels:**
- `none` - No instrumentation
- `info` - Basic span creation with function name
- `debug` - Include SQL query in span (if include_sql is true)
- `trace` - Include both SQL query and parameters in span

**Query Analysis Features:**
- **Sequential scan detection**: Automatically detects queries that perform full table scans
- **Warnings during build**: Identifies queries that might benefit from indexing

### Query Configuration

Each query is defined in its own `.sql` file: `queries/{module}/{query_name}.sql`

The metadata block supports these options:

#### Minimal Example

```sql
-- @automodel
-- @end

SELECT id, name FROM users WHERE id = #{id}
```

If no metadata is provided, sensible defaults are used.

#### All Available Options

```sql
-- @automodel
--    description: Retrieve a user by their ID  # Function documentation
--    module: custom_module    # Override directory-based module name
--    expect: exactly_one       # exactly_one | possible_one | at_least_one | multiple
--    types:                    # Custom type mappings
--      profile: "crate::models::UserProfile"                         # query params/output by name
--      public.positive_int: "std::num::NonZeroI32"                   # domain type alias override
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"  # composite type field
--    telemetry:                # Per-query telemetry settings
--      level: trace
--      include_params: [id, name]
--      include_sql: false
--    ensure_indexes: true      # Enable performance analysis
--    multiunzip: false         # Enable for UNNEST-based batch inserts
--    conditions_type: false    # Use old/new struct for conditional queries
--    parameters_type: false    # Group all parameters into one struct
--    return_type: "UserInfo"   # Custom return type name
--    error_type: "UserError"   # Custom error type name
--    conditions_type_derives:  # Additional derives for conditions struct
--      - serde::Serialize
--    parameters_type_derives:  # Additional derives for parameters struct
--      - serde::Deserialize
--    return_type_derives:      # Additional derives for return struct
--      - serde::Serialize
--      - PartialEq
--    error_type_derives:       # Additional derives for error enum
--      - serde::Serialize
-- @end

SELECT id, name FROM users WHERE id = #{id}
```

### Expected Result Types

Controls how the query is executed and what it returns:

```sql
-- @automodel
--    expect: exactly_one    # fetch_one() -> Result<T, Error> - Fails if 0 or >1 rows
-- @end

-- @automodel
--    expect: possible_one   # fetch_optional() -> Result<Option<T>, Error> - 0 or 1 row
-- @end

-- @automodel
--    expect: at_least_one   # fetch_all() -> Result<Vec<T>, Error> - Fails if 0 rows
-- @end

-- @automodel
--    expect: multiple       # fetch_all() -> Result<Vec<T>, Error> - 0 or more rows (default for collections)
-- @end
```

### Custom Type Mappings

Override PostgreSQL-to-Rust type mappings for specific fields:

```sql
-- @automodel
--    types:
--      profile: "crate::models::UserProfile"  # For input parameters and output fields with this name
--      users.profile: "crate::models::UserProfile"  # For output fields from specific table (when using JOINs)
--      posts.metadata: "crate::models::PostMetadata"
--      status: "UserStatus"  # Custom enum types
--      category: "crate::enums::Category"
-- @end

SELECT id, name, profile FROM users WHERE id = #{id}
```

**JSON Wrapper Control:**

By default, custom types use JSON serialization. Control this with suffixes:

```sql
-- @automodel
--    types:
--      profile: "UserProfile@json"        # Force JSON wrapper (default)
--      uuid: "MyUuid@native"              # No wrapper - type implements sqlx traits
--      data: "Vec<Option<i32>>@native"    # Native binding for complex types
-- @end
```

- **`@native`**: Type implements `sqlx::Encode`/`Decode` (or `tokio_postgres::ToSql`/`FromSql`)
- **`@json`** or no suffix: Uses JSON serialization (requires `serde::Serialize`/`Deserialize`)

**Composite Type Field Mappings:**

Use 3-segment keys (`schema.type.field`) to map fields inside PostgreSQL composite types. This changes the generated struct field type from `serde_json::Value` to your custom Rust type, wrapped in `sqlx::types::Json<T>`:

```sql
-- @automodel
--    types:
--      public.user_with_links_input.social_links: "Vec<crate::models::UserSocialLink>"
-- @end

INSERT INTO public.users (name, email, social_links)
SELECT r.name, r.email, r.social_links
FROM UNNEST(#{items}::public.user_with_links_input[]) AS r(name, email, social_links)
RETURNING id, name, email, social_links
```

This generates the composite type struct with a typed field instead of `serde_json::Value`:

```rust
#[derive(sqlx::Type)]
#[sqlx(type_name = "user_with_links_input")]
pub struct UserWithLinksInput {
    pub name: Option<String>,
    pub email: Option<String>,
    pub social_links: Option<Vec<UserSocialLink>>,
}
```

Key details:

- **`jsonb` fields** → wrapped as `Json<T>` (e.g., `Option<Json<Vec<UserSocialLink>>>`)
- **`jsonb[]` fields** → per-element wrapping as `Vec<Json<T>>` (e.g., `Vec<Option<Json<UserTag>>>`)
- Works for both standalone composite types (`CREATE TYPE`) and table-backed types
- The `@json`/`@native` suffixes apply here too
- Mappings are **global**: if two queries reference the same composite type field, both must specify the same target type (conflicting mappings produce a build error)
- Multiple queries can contribute mappings for different fields of the same composite type

```sql
-- Both queries map the same composite type field — types must agree
-- Query A:
--    types:
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"

-- Query B:
--    types:
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"  # OK: same type
--      public.users.profile: "crate::models::UserProfile"               # OK: different field
```

**Domain Type Alias Mappings:**

PostgreSQL domain types (`CREATE DOMAIN`) are detected automatically and generated as Rust type aliases:

```sql
CREATE DOMAIN positive_int AS INTEGER CHECK (VALUE > 0);
CREATE DOMAIN email_address AS VARCHAR(255) CHECK (VALUE ~* '^[^@]+@[^@]+$');
```

Generated (default):
```rust
pub type PositiveInt = i32;
pub type EmailAddress = String;
```

Use 2-segment keys (`schema.domain_name`) in `types:` to override the alias target:

```sql
-- @automodel
--    types:
--      public.positive_int: "std::num::NonZeroI32"
-- @end
```

Generated (with override):
```rust
pub type PositiveInt = std::num::NonZeroI32;
```

Domain CHECK constraints are also included in error type enums for mutation queries (e.g., `PositiveIntCheck`, `EmailAddressCheck`).

**Type mapping key summary:**

| Key format | Segments | Purpose | Example |
|-----------|----------|---------|---------|
| `field_name` | 1 | Map parameter/column by name | `profile: "UserProfile"` |
| `schema.domain` | 2 | Override domain type alias | `public.positive_int: "NonZeroI32"` |
| `schema.type.field` | 3 | Map composite type field | `public.users.social_links: "Vec<Link>"` |

### Named Parameters

Use `#{parameter_name}` syntax in SQL queries:

```sql
SELECT * FROM users WHERE id = #{user_id} AND status = #{status}
```

**Optional Parameters:**
Add `?` suffix for optional parameters that become `Option<T>`:

```sql
SELECT * FROM posts 
WHERE user_id = #{user_id} 
  AND (#{category?} IS NULL OR category = #{category?})
```

**Optional + Nullable Parameters (`??`):**
Use `??` suffix in conditional blocks when a parameter is both optional (controls block inclusion) and nullable (can set the column to NULL). Generates `Option<Option<T>>`:

```sql
UPDATE users 
SET updated_at = NOW() 
  #[, age = #{age??}] 
WHERE id = #{user_id} 
RETURNING *
```

- `None` → skip the conditional block entirely (no change)  
- `Some(None)` → include the block, set value to NULL  
- `Some(Some(35))` → include the block, set value to 35

**Array Parameters with Nullable Elements (`[?]`):**
Use `[?]` suffix for array parameters where individual elements can be NULL, resulting in `Vec<Option<T>>`:

```sql
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  #{names}::text[],
  #{emails}::text[],
  #{ages[?]}::int4[]  -- Vec<Option<i32>>: array where elements can be NULL
)
```

**Parameter Suffix Reference:**

| Suffix | Generated Type | Use Case |
|--------|---------------|----------|
| (none) | `T` | Required parameter |
| `?` | `Option<T>` | Optional / conditional block parameter |
| `??` | `Option<Option<T>>` | Conditional block + nullable (skip / set NULL / set value) |
| `[?]` | `Vec<Option<T>>` | Array with nullable elements |
| `?[?]` | `Option<Vec<Option<T>>>` | Optional array with nullable elements |
| `??[?]` | `Option<Option<Vec<Option<T>>>>` | Conditional + nullable array with nullable elements |

Suffixes are orthogonal and compose: `?` controls optionality, second `?` adds value nullability, `[?]` adds element nullability.

> **Note:** Top-level `Option<>` in type mappings is banned. Use suffix annotations instead. If a custom type mapping like `Vec<Option<T>>` already has nullable elements, the `[?]` suffix is a no-op (no double-wrapping).

### Exclusive Choice Blocks (Case Syntax)

Additive [conditional blocks](#conditional-queries) (`#[ ... ]`) are AND-combined, so several can apply at once. Sometimes, though, the caller must pick **exactly one** of several mutually-exclusive branches (e.g. a sort mode, or one of several keyset-pagination orderings). Marking each block with a selector directive turns the group into a single generated Rust `enum` argument, so illegal combinations become unrepresentable.

A choice block starts with a directive as its very first token:

```
#[#{selector=variant!} ...sql...]     -- required: arg is `selector: Enum`
#[#{selector=variant?} ...sql...]     -- optional: arg is `selector: Option<Enum>`
```

- `selector` is the shared group name (same across all branches) and becomes the function argument name.
- `variant` is the branch name and becomes an enum variant.
- The trailing marker is mandatory: `!` makes the choice required, `?` makes it optional (where `None` selects the base query with all blocks removed).

Example — keyset pagination over four mutually-exclusive sort modes:

```sql
-- @automodel
--    description: Keyset pagination with mutually-exclusive sort modes
--    expect: multiple
-- @end
SELECT id, name, email, updated_at
FROM users
WHERE 1 = 1
#[#{sort=ua_asc!}   AND (updated_at, id) > (#{cursor_ts?}, #{cursor_id?}) ORDER BY updated_at ASC,  id ASC  LIMIT #{page_size?}]
#[#{sort=ua_desc!}  AND (updated_at, id) < (#{cursor_ts?}, #{cursor_id?}) ORDER BY updated_at DESC, id DESC LIMIT #{page_size?}]
#[#{sort=name_asc!}                                                        ORDER BY name ASC,       id ASC  LIMIT #{page_size?}]
#[#{sort=name_desc!}                                                       ORDER BY name DESC,      id DESC LIMIT #{page_size?}]
```

This generates an enum and a single `sort` argument:

```rust
pub enum GetUsersMultiSortCursorSort {
    UaAsc { cursor_ts: chrono::DateTime<chrono::Utc>, cursor_id: i32, page_size: i64 },
    UaDesc { cursor_ts: chrono::DateTime<chrono::Utc>, cursor_id: i32, page_size: i64 },
    NameAsc { page_size: i64 },
    NameDesc { page_size: i64 },
}

pub async fn get_users_multi_sort_cursor(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    sort: GetUsersMultiSortCursorSort,
) -> Result<Vec<GetUsersMultiSortCursorItem>, /* ... */> { /* ... */ }
```

```rust
// The caller picks exactly one branch; the compiler enforces that each
// variant's required fields are supplied.
let page = get_users_multi_sort_cursor(
    pool,
    GetUsersMultiSortCursorSort::UaAsc { cursor_ts, cursor_id, page_size: 20 },
).await?;
```

**Parameter placement is simple:**

- Every parameter referenced **inside a branch** becomes a **field on that enum variant** — even if the same parameter name appears in more than one branch (like `page_size`), each variant gets its own field.
- Only ungrouped/additive parameters and other groups' selectors become top-level function arguments.

**Rules** (enforced at build time with clear errors):

- A query may declare **multiple independent choice groups** (see below); each distinct selector name becomes its own enum argument.
- A choice group may **coexist with additive `#[...]` blocks** in the same query (see below); blocks without a directive keep their additive `Option<T>` behavior.
- A branch may contain **nested additive `#[...]` blocks** (see below); their parameters become `Option<T>` fields, included only when `Some(...)`.
- A single branch may **span multiple blocks** (see below): repeat its directive on each fragment that must switch together — e.g. an output column and its matching `JOIN` — and they are included or dropped as a unit.
- All branches of a group must use the **same** optionality marker (`!` or `?`).
- Variant names within a group must be unique.
- A parameter may belong to **at most one** choice group (it cannot be shared across two different groups).

#### Mixing choice blocks with additive blocks

A choice group can share a query with ordinary additive `#[...]` blocks — for example additive `WHERE` filters plus an enum-selected sort mode:

```sql
SELECT id, name, email FROM users
WHERE id >= #{min_id}
  #[AND name LIKE #{name_starts_with?}]   -- additive: Option<String> argument
  #[AND age >= #{age_from?}]              -- additive: Option<i32> argument
#[#{sort=unsorted!}  LIMIT #{limit?}]
#[#{sort=name_asc!}  ORDER BY name ASC,  id ASC  LIMIT #{limit?}]
#[#{sort=name_desc!} ORDER BY name DESC, id DESC LIMIT #{limit?}]
```

Here the additive filters stay optional and combine freely, while `sort` is a required enum picking exactly one ordering. Because `limit` is referenced inside every branch, each variant carries its own `limit` field:

```rust
pub enum SearchUsersSort {
    Unsorted { limit: i64 },
    NameAsc { limit: i64 },
    NameDesc { limit: i64 },
}

pub async fn search_users(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    min_id: i32,
    name_starts_with: Option<String>,
    age_from: Option<i32>,
    sort: SearchUsersSort,
) -> Result<Vec<SearchUsersItem>, /* ... */> { /* ... */ }
```

When mixing, every parameter referenced inside a branch becomes a per-variant field on that branch. A branch may also carry **no** parameters at all (e.g. `#[#{sort=unsorted!} LIMIT 100]` with a hardcoded limit), in which case it generates a plain unit variant.

#### Multiple choice groups in one query

A query may declare several independent choice groups, each with its own selector name. Each group compiles to its own enum argument and is selected independently — for example an optional age-`range` group alongside a required `sort` group:

```sql
SELECT id, name, email, age FROM users
WHERE email LIKE #{email_prefix}
  #[#{range=min?} AND age >= #{min_age?}]
  #[#{range=max?} AND age <= #{max_age?}]
#[#{sort=asc!}  ORDER BY id ASC  LIMIT #{lim?}]
#[#{sort=desc!} ORDER BY id DESC LIMIT #{lim?}]
```

`range` is optional (`?`, so `None` applies no age bound) and each branch carries its own field, while `sort` is required (`!`) and shares `lim` across both branches:

```rust
pub enum MultiGroupSearchRange { Min { min_age: i32 }, Max { max_age: i32 } }
pub enum MultiGroupSearchSort  { Asc, Desc }

pub async fn multi_group_search(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    email_prefix: String,
    range: Option<MultiGroupSearchRange>,
    sort: MultiGroupSearchSort,
    lim: i64,
) -> Result<Vec<MultiGroupSearchItem>, /* ... */> { /* ... */ }
```

The only restriction is that a given parameter name may not be shared between two different groups, since the generator numbers and binds each parameter exactly once.

#### Nested optional blocks inside a branch

A choice branch may contain **nested additive `#[...]` blocks**. The branch itself is still selected by the enum, but each nested block is included only when its parameter is `Some(...)` — exactly like a top-level additive block, just scoped to the chosen variant. Nested-block parameters become **`Option<T>` fields** on the variant, while the branch's direct parameters stay plain (non-`Option`) fields.

This is ideal for **keyset pagination where the first page has no cursor** but later pages do: expose the cursor bounds as nested optional fields, so `None` yields the first page (no keyset predicate) and `Some` yields the filtered next page — all from a single source line per sort mode:

```sql
SELECT id, name, email, updated_at
FROM users
WHERE name LIKE #{name_prefix}
  #[#{sort=name_asc?}  #[AND (name, id) > (#{cur_val?}, #{cur_id?})] ORDER BY name ASC,  id ASC]
  #[#{sort=name_desc?} #[AND (name, id) < (#{cur_val?}, #{cur_id?})] ORDER BY name DESC, id DESC]
LIMIT #{lim};
```

Each variant carries its cursor bounds as `Option<T>` fields:

```rust
pub enum PageUsersSort {
    NameAsc  { cur_val: Option<String>, cur_id: Option<i32> },
    NameDesc { cur_val: Option<String>, cur_id: Option<i32> },
}

// First page: no cursor -> keyset predicate omitted.
let first = page_users(pool, prefix.clone(), 20,
    Some(PageUsersSort::NameAsc { cur_val: None, cur_id: None })).await?;

// Next page: pass the last row of the previous page as the cursor.
let last = first.last().unwrap();
let next = page_users(pool, prefix, 20,
    Some(PageUsersSort::NameAsc { cur_val: Some(last.name.clone()), cur_id: Some(last.id) })).await?;
```

A single branch may hold **several independent nested blocks**, and nested blocks can be freely combined with mandatory direct parameters in the same branch:

```sql
SELECT id, name, email, age, is_active FROM users
WHERE name LIKE #{name_prefix}
  #[#{filter=by_active!} AND is_active = #{want_active} #[AND age >= #{active_min_age?}]]
  #[#{filter=by_age!}    AND age >= #{floor_age}        #[AND age <= #{ceil_age?}]]
LIMIT #{lim};
```

```rust
pub enum FilterUsersFilter {
    ByActive { want_active: bool, active_min_age: Option<i32> },  // direct + nested
    ByAge    { floor_age: i32,   ceil_age: Option<i32> },         // direct + nested
}
```

Here `want_active` / `floor_age` are always bound (plain fields), while `active_min_age` / `ceil_age` gate their nested predicate and are only applied when `Some(...)`.

#### Coordinated output: conditional columns, joins, and collections

The examples above vary *how a query filters and sorts*. The same selector mechanism can also vary **what a query returns** — including or omitting an output column, a `JOIN`, or a whole nested entity — while keeping the **result shape fixed** (every branch produces the same columns, so no per-branch row mapping is needed).

The enabler is that one branch may **span several blocks**: repeat the *same* `#{selector=variant!}` directive on each fragment that must switch together. AutoModel merges them into a single branch, so a projection fragment and its matching `JOIN` are included or dropped as a unit.

**Conditional column + coordinated join.** A caller flag decides whether each row also carries a value from a self-join; when off, the `LEFT JOIN` is skipped entirely and the column comes back `NULL`:

```sql
SELECT
  u.id,
  u.name,
  #[#{referrer=on!} r.age]#[#{referrer=off!} NULL] AS referrer_age
FROM public.users u
#[#{referrer=on!} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
ORDER BY u.id
```

Both `on` fragments (the `r.age` projection and the `LEFT JOIN`) belong to the same branch, so they toggle together. `referrer_age` is always present in the struct as `Option<i32>`:

```rust
pub enum UserOptionalReferrerReferrer { On, Off }

pub async fn user_optional_referrer(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    email_prefix: String,
    referrer: UserOptionalReferrerReferrer,
) -> Result<Vec<UserOptionalReferrerItem>, /* ... */> { /* ... */ }
```

> **Author the data branch first** (`on` before `off`): AutoModel infers output types from the first branch that yields columns. Project the off-branch as a literal (`NULL`, or a typed `NULL::some_type`) so the removed join's aliases vanish with it.

**Whole nested entity (composite, no JSON).** Project the entire joined row as a native composite that maps to a generated struct — the field is `Option<T>` because a row expression is inherently nullable:

```sql
SELECT u.id, u.name,
  #[#{referrer=on!} r]#[#{referrer=off!} NULL] AS referrer
FROM public.users u
#[#{referrer=on!} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
-- referrer: Option<types::public::Users>
```

**Collection of children (no JSON aggregate).** Use `array_agg` over a child table's implicit composite type; it decodes straight into `Vec<Struct>`. :

```sql
SELECT u.id, u.name,
  #[#{posts=on!} (SELECT array_agg(p ORDER BY p.id) FROM public.posts p WHERE p.author_id = u.id)]#[#{posts=off!} NULL] AS posts
FROM public.users u
WHERE u.email LIKE #{email_prefix}
-- posts: Option<Vec<types::public::Posts>>
```

**Two selectors, one query.** Independent selectors compose freely — every On/Off combination yields a valid, fixed-shape row. Identical `NULL` off-branches across different groups do **not** collide; blocks are matched positionally, not by their body text:

```sql
SELECT u.id, u.name,
  #[#{referrer=on!} r]#[#{referrer=off!} NULL] AS referrer,
  #[#{posts=on!} (SELECT array_agg(p ORDER BY p.id) FROM public.posts p WHERE p.author_id = u.id)]#[#{posts=off!} NULL] AS posts
FROM public.users u
#[#{referrer=on!} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
-- args: referrer: ...Referrer, posts: ...Posts  (two independent enums)
```

### Non-Null Column Override

By default, expression columns (computed values, function results, literals) are generated as `Option<T>` because PostgreSQL's prepared-statement metadata doesn't report nullability for expressions — only for direct table columns with `NOT NULL` constraints.

When you know an expression result can never be null, use the `!` suffix to override this and generate a non-nullable type:

**Native syntax — `{column_name!}`:**

```sql
-- count(*) is always non-null, generates i64 instead of Option<i64>
SELECT count(*) AS {total!} FROM users

-- Boolean literal is always non-null, generates bool instead of Option<bool>
UPDATE users SET name = #{name} WHERE id = #{id}
RETURNING true AS {applied!}

-- Comparison of NOT NULL columns, generates bool instead of Option<bool>
SELECT created_at > now() - interval '1 year' AS {is_recent!}
FROM users WHERE id = #{id}
```

**sqlx-compatible syntax — `"column_name!"`:**

For easy migration from sqlx, the quoted-identifier syntax is also supported:

```sql
SELECT expires_at > now() AS "is_unexpired!" FROM sessions WHERE id = #{id}
```

**Both syntaxes can be mixed in the same query:**

```sql
SELECT
    id AS {user_id!},
    name AS {user_name!},
    created_at > now() - interval '1 year' AS {is_recent!},
    true AS "is_active!"
FROM users
```

Both syntaxes are rewritten to clean SQL at build time — the `!`, `{`, `}`, and surrounding quotes are stripped from the runtime query sent to PostgreSQL. The override only affects the generated Rust type (removing the `Option<>` wrapper); it does not change query behavior.

### Per-Query Telemetry Configuration

Override global telemetry settings for specific queries in the metadata block:

```sql
-- @automodel
--    telemetry:
--      level: trace              # none | info | debug | trace
--      include_params: [user_id, email]  # Only these parameters logged
--      include_sql: true         # Include SQL in spans
-- @end

SELECT * FROM users WHERE id = #{user_id}
```

### Per-Query Analysis Configuration

Override global analysis settings for specific queries:

```sql
-- @automodel
--    ensure_indexes: true   # Enable/disable analysis for this query
-- @end

SELECT * FROM users WHERE email = #{email}
```

### Module Organization

Generated functions are organized into modules based on directory structure:

```
queries/
├── users/              # Generated as src/generated/users.rs
│   ├── get_user.sql
│   └── create_user.sql
├── posts/              # Generated as src/generated/posts.rs
│   └── get_post.sql
└── admin/              # Generated as src/generated/admin.rs
    └── health_check.sql
```

You can override the module name in the metadata:

```sql
-- @automodel
--    module: custom_module  # Override directory-based module name
-- @end
```

### Complete Examples

**Simple query with custom type:**

`queries/users/get_user_profile.sql`:
```sql
-- @automodel
--    description: Get user profile with custom JSON type
--    expect: possible_one
--    types:
--      profile: "crate::models::UserProfile"
--    telemetry:
--      level: trace
--      include_params: [user_id]
--      include_sql: true
--    ensure_indexes: true
-- @end

SELECT id, name, profile 
FROM users 
WHERE id = #{user_id}
```

**Query with optional parameter:**

`queries/posts/search_posts.sql`:
```sql
-- @automodel
--    description: Search posts with optional category filter
--    expect: multiple
--    types:
--      category: "PostCategory"
--      metadata: "crate::models::PostMetadata"
--    ensure_indexes: true
-- @end

SELECT * FROM posts 
WHERE user_id = #{user_id} 
  AND (#{category?} IS NULL OR category = #{category?})
```

**DDL query without analysis:**

`queries/setup/create_sessions_table.sql`:
```sql
-- @automodel
--    description: Create sessions table
--    ensure_indexes: false
-- @end

CREATE TABLE IF NOT EXISTS sessions (
  id UUID PRIMARY KEY, 
  created_at TIMESTAMPTZ DEFAULT NOW()
)
```

**Bulk operation with minimal telemetry:**

`queries/admin/cleanup_old_sessions.sql`:
```sql
-- @automodel
--    description: Remove sessions older than cutoff date
--    expect: exactly_one
--    telemetry:
--      include_params: []  # Skip all parameters for privacy
--      include_sql: false
-- @end

DELETE FROM sessions 
WHERE created_at < #{cutoff_date}
```

## Conditional Queries

AutoModel supports **conditional queries** that dynamically include or exclude SQL clauses based on parameter availability. This allows you to write flexible queries that adapt based on which optional parameters are provided.

### Conditional Syntax

Use the `#[...]` syntax to wrap optional SQL parts:

`queries/users/search_users.sql`:
```sql
-- @automodel
--    description: Search users with optional name and age filters
-- @end

SELECT id, name, email 
FROM users 
WHERE 1=1 
  #[AND name ILIKE #{name_pattern?}] 
  #[AND age >= #{min_age?}] 
ORDER BY created_at DESC
```

**Key Components:**
- `#[AND name ILIKE #{name_pattern?}]` - Conditional block that includes the clause only if `name_pattern` is `Some`
- `#{name_pattern?}` - Optional parameter (note the `?` suffix)
- The conditional block is removed entirely if the parameter is `None`

### Runtime SQL Examples

The same function generates different SQL based on parameter availability:

```rust
// Both parameters provided
search_users(executor, Some("%john%".to_string()), Some(25)).await?;
// SQL: "SELECT id, name, email FROM users WHERE 1=1 AND name ILIKE $1 AND age >= $2 ORDER BY created_at DESC"
// Params: ["%john%", 25]

// Only name pattern provided  
search_users(executor, Some("%john%".to_string()), None).await?;
// SQL: "SELECT id, name, email FROM users WHERE 1=1 AND name ILIKE $1 ORDER BY created_at DESC"
// Params: ["%john%"]

// Only age provided
search_users(executor, None, Some(25)).await?;
// SQL: "SELECT id, name, email FROM users WHERE 1=1 AND age >= $1 ORDER BY created_at DESC"  
// Params: [25]

// No optional parameters
search_users(executor, None, None).await?;
// SQL: "SELECT id, name, email FROM users WHERE 1=1 ORDER BY created_at DESC"
// Params: []
```

### Complex Conditional Queries

You can mix conditional and non-conditional parameters:

`queries/users/find_users_complex.sql`:
```sql
-- @automodel
--    description: Complex search with required name pattern and optional filters
-- @end

SELECT id, name, email, age 
FROM users 
WHERE name ILIKE #{name_pattern} 
  #[AND age >= #{min_age?}] 
  AND email IS NOT NULL 
  #[AND created_at >= #{since?}] 
ORDER BY name
```

This generates a function with signature:
```rust
pub async fn find_users_complex(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    name_pattern: String,        // Required parameter
    min_age: Option<i32>,        // Optional parameter
    since: Option<chrono::DateTime<chrono::Utc>>  // Optional parameter
) -> Result<Vec<FindUsersComplexItem>, super::ErrorReadOnly>
```

### Best Practices

1. **Use `WHERE 1=1`** as a base condition when all WHERE clauses are conditional:
   ```sql
   SELECT * FROM users 
   WHERE 1=1 
     #[AND name = #{name?}] 
     #[AND age > #{min_age?}]
   ```

### Conditional UPDATE Statements

Conditional syntax is also useful for UPDATE statements where you want to update only certain fields based on which parameters are provided:

`queries/users/update_user_fields.sql`:
```sql
-- @automodel
--    description: Update user fields conditionally - only updates fields that are provided (not None)
--    expect: exactly_one
-- @end

UPDATE users 
SET updated_at = NOW() 
  #[, name = #{name?}] 
  #[, email = #{email?}] 
  #[, age = #{age?}] 
WHERE id = #{user_id} 
RETURNING id, name, email, age, updated_at
```

This generates a function that allows partial updates:

```rust
// Update only the name
update_user_fields(executor, user_id, Some("Jane Doe".to_string()), None, None).await?;
// SQL: "UPDATE users SET updated_at = NOW(), name = $1 WHERE id = $2 RETURNING ..."

// Update only the age  
update_user_fields(executor, user_id, None, None, Some(35)).await?;
// SQL: "UPDATE users SET updated_at = NOW(), age = $1 WHERE id = $2 RETURNING ..."

// Update multiple fields
update_user_fields(executor, user_id, Some("Jane".to_string()), Some("jane@example.com".to_string()), None).await?;
// SQL: "UPDATE users SET updated_at = NOW(), name = $1, email = $2 WHERE id = $3 RETURNING ..."

// Update all fields
update_user_fields(executor, user_id, Some("Janet".to_string()), Some("janet@example.com".to_string()), Some(40)).await?;
// SQL: "UPDATE users SET updated_at = NOW(), name = $1, email = $2, age = $3 WHERE id = $4 RETURNING ..."
```

**Note**: Always include at least one non-conditional SET clause (like `updated_at = NOW()`) to ensure the UPDATE statement is syntactically valid even when all optional parameters are `None`.

## Struct Configuration and Reuse

AutoModel provides four powerful configuration options that allow you to customize how structs and error types are generated and reused across queries: `parameters_type`, `conditions_type`, `return_type`, and `error_type`. These options enable you to eliminate code duplication, improve type safety, and create cleaner APIs.

### Overview

| Option | Purpose | Default | Accepts | Generates |
|--------|---------|---------|---------|-----------|
| `parameters_type` | Group query parameters into a struct | `false` | `true` or struct name | `{QueryName}Params` struct |
| `conditions_type` | Diff-based conditional parameters | `false` | `true` or struct name | `{QueryName}Params` struct with old/new comparison |
| `return_type` | Custom name for return type struct | auto | struct name or omit | Custom named or `{QueryName}Item` struct |
| `error_type` | Custom name for error constraint enum (mutations only) | auto | error type name or omit | Custom named or `{QueryName}Constraints` enum |

Any structure or error type generated can be referenced by other queries. AutoModel validates at build time that the types are compatible and constraints match exactly.

### parameters_type: Structured Parameters

Group all query parameters into a single struct instead of passing them individually. Makes function calls cleaner and enables parameter reuse.

**Basic Usage:**

`queries/users/insert_user_structured.sql`:
```sql
-- @automodel
--    parameters_type: true  # Generates InsertUserStructuredParams
-- @end

INSERT INTO users (name, email, age) 
VALUES (#{name}, #{email}, #{age}) 
RETURNING id
```

**Generated Code:**

```rust
#[derive(Debug, Clone)]
pub struct InsertUserStructuredParams {
    pub name: String,
    pub email: String,
    pub age: i32,
}

pub async fn insert_user_structured(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    params: &InsertUserStructuredParams
) -> Result<i32, super::Error<InsertUserStructuredConstraints>>
```

**Usage:**

```rust
let params = InsertUserStructuredParams {
    name: "Alice".to_string(),
    email: "alice@example.com".to_string(),
    age: 30,
};
insert_user_structured(executor, &params).await?;
```

**Struct Reuse:**

Specify an existing struct name to reuse it across queries:

`queries/users/get_user_by_id_and_email.sql`:
```sql
-- @automodel
--    parameters_type: true  # Generates GetUserByIdAndEmailParams
-- @end

SELECT id, name, email FROM users WHERE id = #{id} AND email = #{email}
```

`queries/users/delete_user_by_id_and_email.sql`:
```sql
-- @automodel
--    parameters_type: "GetUserByIdAndEmailParams"  # Reuses existing struct
-- @end

DELETE FROM users WHERE id = #{id} AND email = #{email} RETURNING id
```

Only one struct definition is generated, shared by both functions.

### conditions_type: Diff-Based Conditional Parameters

For queries with conditional SQL (`#[...]` blocks), generate a struct and compare old vs new values to decide which clauses to include. Works with any query type (SELECT, UPDATE, DELETE, etc.).

**Basic Usage:**

`queries/users/update_user_fields_diff.sql`:
```sql
-- @automodel
--    conditions_type: true  # Generates UpdateUserFieldsDiffParams
-- @end

UPDATE users 
SET updated_at = NOW() 
  #[, name = #{name?}] 
  #[, email = #{email?}] 
WHERE id = #{user_id}
```

**Generated Code:**

```rust
pub struct UpdateUserFieldsDiffParams {
    pub name: String,
    pub email: String,
}

pub async fn update_user_fields_diff(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    old: &UpdateUserFieldsDiffParams,
    new: &UpdateUserFieldsDiffParams,
    user_id: i32
) -> Result<(), super::Error<UpdateUserFieldsDiffConstraints>>
```

**Usage:**

```rust
let old = UpdateUserFieldsDiffParams {
    name: "Alice".to_string(),
    email: "alice@example.com".to_string(),
};
let new = UpdateUserFieldsDiffParams {
    name: "Alicia".to_string(),  // Changed
    email: "alice@example.com".to_string(),  // Same
};
update_user_fields_diff(executor, &old, &new, 42).await?;
// Only executes: UPDATE users SET updated_at = NOW(), name = $1 WHERE id = $2
```

**How It Works:**
- The struct contains only conditional parameters (those ending with `?` or `??`)
- Non-conditional parameters remain as individual function parameters
- At runtime, the function compares `old.field != new.field`
- Only clauses where the field differs are included in the query

**Nullable Fields with `??`:**

Use `??` in conditional blocks when a field is nullable (e.g., `age` column that allows NULL):

```sql
-- @automodel
--    conditions_type: true
-- @end

UPDATE users 
SET updated_at = NOW() 
  #[, name = #{name?}] 
  #[, age = #{age??}] 
WHERE id = #{user_id}
```

```rust
pub struct UpdateUserParamsParams {
    pub name: String,          // ? → non-nullable field
    pub age: Option<i32>,      // ?? → nullable field (can be set to NULL)
}
```

With `conditions_type`, the diff comparison works naturally: if `old.age != new.age`, the clause is included — and `new.age` being `None` means "set to NULL".

**Struct Reuse:**

`queries/users/update_user_profile_diff.sql`:
```sql
-- @automodel
--    conditions_type: true
-- @end

UPDATE users 
SET updated_at = NOW() 
  #[, name = #{name?}] 
  #[, email = #{email?}] 
WHERE id = #{user_id}
```

`queries/users/update_user_metadata_diff.sql`:
```sql
-- @automodel
--    conditions_type: "UpdateUserProfileDiffParams"  # Reuses existing diff struct
-- @end

UPDATE users 
SET updated_at = NOW() 
  #[, name = #{name?}] 
  #[, email = #{email?}] 
WHERE id = #{user_id}
```

### return_type: Custom Return Type Names

Customize the name of return type structs (generated for multi-column SELECT queries) and enable struct reuse across queries.

**Basic Usage:**

`queries/users/get_user_summary.sql`:
```sql
-- @automodel
--    return_type: "UserSummary"  # Custom name instead of GetUserSummaryItem
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

**Generated Code:**

```rust
#[derive(Debug, Clone)]
pub struct UserSummary {
    pub id: i32,
    pub name: String,
    pub email: String,
}

pub async fn get_user_summary(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: i32
) -> Result<UserSummary, super::ErrorReadOnly>
```

**Struct Reuse:**

Multiple queries returning the same columns can share the same struct:

`queries/users/get_user_summary.sql`:
```sql
-- @automodel
--    return_type: "UserSummary"  # Generates the struct
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

`queries/users/get_user_info_by_email.sql`:
```sql
-- @automodel
--    return_type: "UserSummary"  # Reuses the struct
-- @end

SELECT id, name, email FROM users WHERE email = #{email}
```

`queries/users/get_all_user_summaries.sql`:
```sql
-- @automodel
--    return_type: "UserSummary"  # Reuses the struct
-- @end

SELECT id, name, email FROM users ORDER BY name
```

Only one `UserSummary` struct is generated, shared by all three functions.

### Cross-Struct Reuse

You can reuse struct names across queries. AutoModel will:
1. **Auto-generate** if the struct doesn't exist yet (from the first query that uses it)
2. **Reuse** if the struct already exists (from a previous query in the same module)
3. **Validate** that fields match exactly when reusing

`queries/users/get_user_info.sql`:
```sql
-- @automodel
--    return_type: "UserInfo"  # First use: generates UserInfo struct from return columns
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

`queries/users/update_user_info.sql`:
```sql
-- @automodel
--    parameters_type: "UserInfo"  # Second use: reuses existing UserInfo struct for parameters
-- @end

UPDATE users SET name = #{name}, email = #{email} WHERE id = #{id}
```

**Usage:**

```rust
// Get user info
let user = get_user_info(executor, 42).await?;

// Modify and update using the same struct
let updated = UserInfo {
    name: "New Name".to_string(),
    ..user
};
update_user_info(executor, &updated).await?;
```

### Custom Derive Traits

Add additional derive traits to generated structs and enums using `*_derives` options. These are combined with the global defaults configured in your `build.rs`.

#### Global Default Derives

Configure derive traits that apply to all generated types in your `build.rs`:

```rust
let defaults = automodel::DefaultsConfig {
    // ... other config ...
    derives: automodel::DefaultsDerivesConfig {
        return_type: vec!["Clone".to_string()],
        parameters_type: vec!["Clone".to_string()],
        conditions_type: vec!["Clone".to_string()],
        error_type: vec!["Clone".to_string()],
    },
};
```

This ensures all generated structs include `Clone` in addition to the always-present `Debug` trait.

#### Per-Query Additional Derives

Add query-specific derive traits that append to the global defaults:

```sql
-- @automodel
--    return_type: "UserId"
--    return_type_derives:
--      - serde::Serialize
--      - serde::Deserialize
--      - PartialEq
--      - Eq
-- @end

SELECT id FROM users WHERE email = #{email}
```

**Generates:**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct UserId {
    pub id: i32,
}
```

Note: `Clone` comes from global defaults, `serde` traits and `PartialEq`/`Eq` from per-query config.

**Available Options:**
- `conditions_type_derives` - For conditions struct (used with `conditions_type`)
- `parameters_type_derives` - For parameters struct (used with `parameters_type`)  
- `return_type_derives` - For return type struct
- `error_type_derives` - For constraint error enum

**Trait Merging:**
- Global defaults are applied first
- Per-query derives are appended
- Duplicates are automatically removed
- `Debug` is always included by default

### Build-Time Validation

AutoModel validates struct field compatibility at build time:

1. **Auto-Generation**: If a named struct doesn't exist, AutoModel automatically generates it from the query
2. **Field Matching**: When reusing an existing struct, query parameters/columns must exactly match struct fields (names and types)
3. **Clear Error Messages**: Validation failures provide helpful guidance

**Example validation errors:**

```
Error: Query parameter 'age' not found in struct 'UserInfo'.
Available fields: id, name, email
```

```
Error: Type mismatch for parameter 'id' in struct 'UserInfo':
expected 'i64', but query requires 'i32'
```

### Struct Definition Sources

Structs can be generated from three sources:

1. **parameters_type: true** → `{QueryName}Params` (input parameters)
2. **conditions_type: true** → `{QueryName}Params` (conditional input parameters)
3. **return_type: "Name"** → Custom named struct (output columns)
4. **Multi-column SELECT** → `{QueryName}Item` (output columns, when return_type not specified)

### When to Use Each Option

**Use `parameters_type`:**
- Queries with 3+ parameters where individual params become unwieldy
- Building query parameters from existing structs or API input
- Reusing parameter sets with slight modifications
- Improving code organization and reducing function signature complexity

**Use `conditions_type`:**
- Conditional queries (`#[...]`) with state comparison logic
- UPDATE queries that should only modify changed fields
- SELECT queries with filters that should only apply when criteria changed
- Implementing PATCH-style REST endpoints
- Avoiding the verbosity of many `Option<T>` parameters

**Use `return_type`:**
- Multiple queries returning the same column structure
- Creating domain-specific struct names (e.g., `UserSummary` instead of `GetUserItem`)
- Reusing return types as input parameters for related queries
- Building consistent DTOs across your API

### Complete Example

`queries/users/get_user_summary.sql`:
```sql
-- @automodel
--    return_type: "UserSummary"  # Define a common return type
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

`queries/users/search_users.sql`:
```sql
-- @automodel
--    return_type: "UserSummary"  # Reuse it in other queries
-- @end

SELECT id, name, email FROM users WHERE name ILIKE #{pattern}
```

`queries/users/update_user_contact.sql`:
```sql
-- @automodel
--    parameters_type: "UserSummary"  # Use it as input parameters
-- @end

UPDATE users SET name = #{name}, email = #{email} WHERE id = #{id}
```

`queries/users/partial_update_user.sql`:
```sql
-- @automodel
--    conditions_type: true  # Generates PartialUpdateUserParams
-- @end

UPDATE users 
SET updated_at = NOW() 
  #[, name = #{name?}] 
  #[, email = #{email?}] 
WHERE id = #{user_id}
```

**Generated Code:**

```rust
// Single struct definition shared across queries
#[derive(Debug, Clone)]
pub struct UserSummary {
    pub id: i32,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct PartialUpdateUserParams {
    pub name: String,
    pub email: String,
}

pub async fn get_user_summary(...) -> Result<UserSummary, super::ErrorReadOnly>
pub async fn search_users(...) -> Result<Vec<UserSummary>, super::ErrorReadOnly>
pub async fn update_user_contact(..., params: &UserSummary) -> Result<(), super::Error<UpdateUserContactConstraints>>
pub async fn partial_update_user(..., old: &PartialUpdateUserParams, new: &PartialUpdateUserParams, ...) -> Result<(), super::Error<PartialUpdateUserConstraints>>
```

### Notes

- **Auto-generation of named structs**: If a struct name is specified but doesn't exist yet, AutoModel generates it automatically
- **Struct reuse from previous queries**: You can reference structs generated by earlier queries in the same module
- **Exact field matching**: When reusing existing structs, all query parameters/columns must match struct fields exactly
- **No subset matching**: You cannot use a struct with extra fields; all fields must match
- **parameters_type ignored when conditions_type is enabled**: Diff-based queries already use structured parameters

## Batch Insert with UNNEST Pattern

AutoModel supports efficient batch inserts using PostgreSQL's `UNNEST` function, which allows you to insert multiple rows in a single query. This is much more efficient than inserting rows one at a time.

### Basic UNNEST Pattern

PostgreSQL's `UNNEST` function can expand multiple arrays into a set of rows:

```sql
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  ARRAY['Alice', 'Bob', 'Charlie'],
  ARRAY['alice@example.com', 'bob@example.com', 'charlie@example.com'],
  ARRAY[25, 30, 35]
)
RETURNING id, name, email, age, created_at;
```

### Using UNNEST with AutoModel

Define a batch insert query in a SQL file:

`queries/users/insert_users_batch.sql`:
```sql
-- @automodel
--    description: Insert multiple users using UNNEST pattern
--    expect: multiple
--    multiunzip: true
-- @end

INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(#{name}::text[], #{email}::text[], #{age}::int4[])
RETURNING id, name, email, age, created_at
```

**Key Points:**
- Use array parameters: `#{name}::text[]`, `#{email}::text[]`, etc.
- Include explicit type casts for proper type inference
- Set `expect: "multiple"` to return a vector of results
- Set `multiunzip: true` to enable the special batch insert mode

### The `multiunzip` Configuration Parameter

When `multiunzip: true` is set, AutoModel generates special code to handle batch inserts more ergonomically:

**Without `multiunzip`** (standard array parameters):
```rust
// You would need to pass separate arrays for each column
insert_users_batch(
    &client,
    vec!["Alice".to_string(), "Bob".to_string()],
    vec!["alice@example.com".to_string(), "bob@example.com".to_string()],
    vec![25, 30]
).await?;
```

**With `multiunzip: true`** (generates a record struct):
```rust
// AutoModel generates an InsertUsersBatchRecord struct
#[derive(Debug, Clone)]
pub struct InsertUsersBatchRecord {
    pub name: String,
    pub email: String,
    pub age: i32,
}

// Now you can pass a single vector of records
insert_users_batch(
    &client,
    vec![
        InsertUsersBatchRecord {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 25,
        },
        InsertUsersBatchRecord {
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            age: 30,
        },
    ]
).await?;
```

### Nullable Elements in Batch Inserts

Both with and without `multiunzip`, you can use the `??` suffix to indicate array elements can be NULL:

**Without multiunzip:**
```sql
-- @automodel
--    expect: multiple
-- @end
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  #{names}::text[],
  #{emails}::text[],
  #{ages??}::int4[]  -- Array where individual elements can be NULL
)
```

Generated function signature:
```rust
pub async fn insert_users(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    names: Vec<String>,
    emails: Vec<String>,
    ages: Vec<Option<i32>>  // Elements can be NULL
) -> Result<Vec<InsertUsersItem>, super::Error<InsertUsersConstraints>>
```

**With multiunzip:**
```sql
-- @automodel
--    expect: multiple
--    multiunzip: true
-- @end
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  #{name}::text[],
  #{email}::text[],
  #{age?}::int4[]  -- Use ? in struct field for optional
)
```

Generated struct with optional field:
```rust
pub struct InsertUsersRecord {
    pub name: String,
    pub email: String,
    pub age: Option<i32>,  // Field is optional
}

// Unpacks to Vec<Option<i32>> via multiunzip
```

### How `multiunzip` Works

When `multiunzip: true` is enabled:

1. **Generates an input record struct** with fields matching your parameters
2. **Uses itertools::multiunzip()** to transform `Vec<Record>` into tuple of arrays `(Vec<name>, Vec<email>, Vec<age>)`
3. **Binds each array** to the corresponding SQL parameter

Generated function signature:
```rust
pub async fn insert_users_batch(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    items: Vec<InsertUsersBatchRecord>  // Single parameter instead of multiple arrays
) -> Result<Vec<InsertUsersBatchItem>, super::Error<InsertUsersBatchConstraints>>
```

Internal implementation:
```rust
use itertools::Itertools;

// Transform Vec<Record> into separate arrays
let (name, email, age): (Vec<_>, Vec<_>, Vec<_>) =
    items
        .into_iter()
        .map(|item| (item.name, item.email, item.age))
        .multiunzip();

// Bind each array to the query
let query = query.bind(name);
let query = query.bind(email);
let query = query.bind(age);
```

### Multiunzip Crate Selection

By default, AutoModel uses `itertools::multiunzip()` which supports up to 12 parameters. For batch inserts with more than 12 columns, you can configure AutoModel to use the `many-unzip` crate instead, which supports up to 196 parameters.

Configure in your `build.rs`:

```rust
let defaults = automodel::DefaultsConfig {
    // ... other config ...
    multiunzip_crate: automodel::MultiunzipCrate::ManyUnzip,  // Use many-unzip instead of itertools
};
```

**When to use which:**
- `MultiunzipCrate::Itertools` (default): For queries with up to 12 parameters. Most common use case.
- `MultiunzipCrate::ManyUnzip`: For queries with 13-196 parameters. Requires adding `many-unzip` to your `Cargo.toml`:

```toml
[dependencies]
many-unzip = "0.1"  # or latest version
```

The generated code automatically uses the correct trait based on your configuration:
- **Itertools**: `use itertools::Itertools;`
- **ManyUnzip**: `use many_unzip::ManyUnzip;`

Both crates provide the same `.multiunzip()` method, so the rest of the generated code remains identical.

### Complete Example

`queries/posts/insert_posts_batch.sql`:
```sql
-- @automodel
--    description: Batch insert multiple posts
--    expect: multiple
--    multiunzip: true
-- @end

INSERT INTO posts (title, content, author_id, published_at)
SELECT * FROM UNNEST(
  #{title}::text[],
  #{content}::text[],
  #{author_id}::int4[],
  #{published_at}::timestamptz[]
)
RETURNING id, title, author_id, created_at
```

**Usage:**
```rust
use crate::generated::posts::{insert_posts_batch, InsertPostsBatchRecord};

let posts = vec![
    InsertPostsBatchRecord {
        title: "First Post".to_string(),
        content: "Content 1".to_string(),
        author_id: 1,
        published_at: chrono::Utc::now(),
    },
    InsertPostsBatchRecord {
        title: "Second Post".to_string(),
        content: "Content 2".to_string(),
        author_id: 1,
        published_at: chrono::Utc::now(),
    },
];

let inserted = insert_posts_batch(&client, posts).await?;
println!("Inserted {} posts", inserted.len());

```

### Array Columns in Batch Inserts (jsonb[], text[], etc.)

PostgreSQL's `UNNEST` flattens multidimensional arrays. This means you **cannot** pass `jsonb[][]` or `text[][]` to insert into a column of type `jsonb[]` or `text[]` — `UNNEST` would flatten the nested arrays into individual elements instead of producing one array per row.

The workaround is to pass each row's array value as a single `jsonb` (a JSON array), then reconstruct the PostgreSQL array in SQL using `jsonb_array_elements`:

**For nullable array columns** (`jsonb[] DEFAULT NULL`):
```sql
-- @automodel
--    expect: multiple
--    multiunzip: true
--    types:
--      tags: "Vec<Option<crate::models::UserTag>>"
--      public.users.tags: "Vec<Option<crate::models::UserTag>>"
-- @end
INSERT INTO public.users (name, email, tags)
SELECT name, email,
    CASE WHEN tags IS NULL THEN NULL
    ELSE ARRAY(SELECT jsonb_array_elements(tags)) END
FROM UNNEST(
        #{name}::text [],
        #{email}::text [],
        #{tags}::jsonb []
    ) AS t(name, email, tags)
RETURNING id, name, email, tags;
```

**For required array columns** (`jsonb[] NOT NULL`):
```sql
-- @automodel
--    expect: multiple
--    multiunzip: true
--    types:
--      labels: "Vec<Option<crate::models::UserTag>>"
--      public.users.labels: "Vec<Option<crate::models::UserTag>>"
-- @end
INSERT INTO public.users (name, email, labels)
SELECT name, email,
    ARRAY(SELECT jsonb_array_elements(labels))
FROM UNNEST(
        #{name}::text [],
        #{email}::text [],
        #{labels}::jsonb []
    ) AS t(name, email, labels)
RETURNING id, name, email, labels;
```

**How it works:**

1. The generated Rust code automatically serializes each row's array value to a `jsonb` value (a JSON array like `[{"label":"rust"},{"label":"go"}]`) — this is transparent to the caller
2. `UNNEST` on `jsonb[]` yields one `jsonb` scalar per row — no flattening
3. `ARRAY(SELECT jsonb_array_elements(tags))` reconstructs the `jsonb[]` from the JSON array
4. For nullable columns, the `CASE WHEN ... IS NULL THEN NULL` guard preserves SQL NULLs

The `types:` annotation maps both the parameter and output column to your custom Rust type (e.g. `Vec<Option<crate::models::UserTag>>`). AutoModel handles serialization/deserialization of each element individually.

> **Why not `jsonb[][]`?** PostgreSQL requires uniform sub-array lengths in multidimensional arrays and `UNNEST` flattens all dimensions. These constraints make `type[][]` unusable for variable-length per-row arrays.

**For plain `text[]` columns** (using `jsonb_array_elements_text` to reconstruct):
```sql
-- @automodel
--    expect: multiple
--    multiunzip: true
-- @end
INSERT INTO public.items (name, tags)
SELECT name,
    ARRAY(SELECT jsonb_array_elements_text(tags))::text[]
FROM UNNEST(
        #{name}::text [],
        #{tags}::jsonb []
    ) AS t(name, tags)
RETURNING id, name, tags;
```

The pattern is the same as for `jsonb[]` — in the SQL, the parameter is declared as `jsonb[]` so that UNNEST receives flat scalars. AutoModel's generated code automatically serializes the Rust `Vec<String>` values to JSON arrays before binding, so the conversion is transparent to the caller. On the SQL side, `jsonb_array_elements_text()` extracts `text` values from each JSON array, and `ARRAY(...)::text[]` reconstructs the `text[]` column.

### UNNEST with Composite Types

As an alternative to the `multiunzip` pattern (where each column is a separate array parameter), you can use **PostgreSQL composite types** with UNNEST. Instead of passing N separate arrays in the SQL, you pass a single array of a composite (row) type. AutoModel auto-generates the corresponding Rust struct with `Encode`, `Decode`, `Type`, and `PgHasArrayType` implementations.

From the caller's perspective, both approaches look the same — you pass a `Vec<SomeStruct>` and get results back. The difference is in how the SQL query is written and what happens under the hood: `multiunzip` splits the struct into separate arrays internally, while composite types bind a single typed array directly to PostgreSQL.

**When to prefer composite types over multiunzip:**
- Your input rows have nested structure (e.g., composite fields within composites)
- You don't want the `itertools` / `many-unzip` crate dependency
- No `multiunzip: true` metadata is needed — the composite type is detected automatically
- You want to leverage PostgreSQL's type system for input validation

**Step 1: Define a composite type in PostgreSQL:**

```sql
CREATE TYPE public.user_with_links_input AS (
    name TEXT,
    email TEXT,
    social_links JSONB
);
```

**Step 2: Write the query using the composite type array:**

`queries/users_array_fields/insert_users_bulk_composite.sql`:
```sql
-- @automodel
--    description: Bulk insert users with social links using composite type UNNEST
--    expect: multiple
--    types:
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"
-- @end

INSERT INTO public.users (name, email, social_links)
SELECT r.name, r.email, r.social_links
FROM UNNEST(#{items}::public.user_with_links_input[]) AS r(name, email, social_links)
RETURNING id, name, email, social_links
```

No `multiunzip: true` is needed. AutoModel detects the composite type from the `::public.user_with_links_input[]` cast and generates:

```rust
// Auto-generated composite type struct with sqlx trait impls
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserWithLinksInput {
    pub name: Option<String>,
    pub email: Option<String>,
    pub social_links: Option<serde_json::Value>,
}

// Function accepts a single Vec of the composite type
pub async fn insert_users_bulk_composite(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    items: Vec<UserWithLinksInput>,
) -> Result<Vec<InsertUsersBulkCompositeItem>, super::Error<InsertUsersBulkCompositeConstraints>>
```

**Step 3: Use in Rust code:**

```rust
use crate::models::UserSocialLink;

let items = vec![
    UserWithLinksInput {
        name: Some("Alice".to_string()),
        email: Some("alice@example.com".to_string()),
        social_links: Some(serde_json::to_value(&vec![
            UserSocialLink { name: "GitHub".to_string(), url: "https://github.com/alice".to_string() },
        ]).unwrap()),
    },
    UserWithLinksInput {
        name: Some("Bob".to_string()),
        email: Some("bob@example.com".to_string()),
        social_links: None, // NULL social_links
    },
];

let results = insert_users_bulk_composite(&pool, items).await?;
```

**Comparison: multiunzip vs composite type UNNEST**

| Aspect | `multiunzip: true` | Composite type |
|--------|-------------------|----------------|
| Rust caller API | `Vec<Record>` | `Vec<CompositeType>` (same feel) |
| SQL parameter style | Separate arrays: `#{name}::text[], #{email}::text[]` | Single array: `#{items}::composite_type[]` |
| Under the hood | Struct split into arrays via `multiunzip()` | Array of composite bound directly to PG |
| Requires DDL | No (uses built-in types) | Yes (`CREATE TYPE`) |
| Metadata config | `multiunzip: true` | None (auto-detected) |
| Nested composites | Not supported | Supported (composites within composites) |
| Dependencies | `itertools` or `many-unzip` crate | None |

Both approaches produce the same result — efficient bulk inserts via a single `INSERT ... SELECT * FROM UNNEST(...)` statement.

## Upsert Pattern (INSERT ... ON CONFLICT)

PostgreSQL's `ON CONFLICT` clause allows you to handle conflicts when inserting data, enabling "upsert" operations (insert if new, update if exists). AutoModel fully supports this pattern for both single-row and batch operations.

### Understanding EXCLUDED

In the `DO UPDATE` clause, `EXCLUDED` is a special table reference provided by PostgreSQL that contains the row that **would have been inserted** if there had been no conflict. This allows you to reference the attempted insert values.

```sql
INSERT INTO users (email, name, age)
VALUES ('alice@example.com', 'Alice', 25)
ON CONFLICT (email)
DO UPDATE SET
  name = EXCLUDED.name,      -- Use the name from the VALUES clause
  age = EXCLUDED.age,        -- Use the age from the VALUES clause
  updated_at = NOW()         -- Set updated_at to current timestamp
```

In this example:
- `EXCLUDED.name` refers to `'Alice'` (the value being inserted)
- `EXCLUDED.age` refers to `25` (the value being inserted)
- `users.name` and `users.age` refer to the existing row's values in the table

You can also mix both references:
```sql
-- Only update if the new age is greater than the existing age
DO UPDATE SET age = EXCLUDED.age WHERE users.age < EXCLUDED.age
```

### Single Row Upsert

Use `ON CONFLICT` to update existing rows when a conflict occurs:

`queries/users/upsert_user.sql`:
```sql
-- @automodel
--    description: Insert a new user or update if email already exists
--    expect: exactly_one
--    types:
--      profile: "crate::models::UserProfile"
-- @end

INSERT INTO users (email, name, age, profile)
VALUES (#{email}, #{name}, #{age}, #{profile})
ON CONFLICT (email) 
DO UPDATE SET 
  name = EXCLUDED.name,
  age = EXCLUDED.age,
  profile = EXCLUDED.profile,
  updated_at = NOW()
RETURNING id, email, name, age, created_at, updated_at
```

**Usage:**
```rust
use crate::generated::users::upsert_user;
use crate::models::UserProfile;

// First insert - creates new user
let user = upsert_user(
    &client,
    "alice@example.com".to_string(),
    "Alice".to_string(),
    25,
    UserProfile { bio: "Developer".to_string() }
).await?;

// Second call with same email - updates existing user
let updated_user = upsert_user(
    &client,
    "alice@example.com".to_string(),
    "Alice Smith".to_string(),  // Updated name
    26,                          // Updated age
    UserProfile { bio: "Senior Developer".to_string() }
).await?;

// Same ID, but updated fields
assert_eq!(user.id, updated_user.id);
```

### Batch Upsert with UNNEST

Combine `UNNEST` with `ON CONFLICT` for efficient batch upserts:

`queries/users/upsert_users_batch.sql`:
```sql
-- @automodel
--    description: Batch upsert users - insert new or update existing by email
--    expect: multiple
--    multiunzip: true
-- @end

INSERT INTO users (email, name, age)
SELECT * FROM UNNEST(
  #{email}::text[],
  #{name}::text[],
  #{age}::int4[]
)
ON CONFLICT (email)
DO UPDATE SET
  name = EXCLUDED.name,
  age = EXCLUDED.age,
  updated_at = NOW()
RETURNING id, email, name, age, created_at, updated_at
```

**Usage:**
```rust
use crate::generated::users::{upsert_users_batch, UpsertUsersBatchRecord};

let users = vec![
    UpsertUsersBatchRecord {
        email: "alice@example.com".to_string(),
        name: "Alice".to_string(),
        age: 25,
    },
    UpsertUsersBatchRecord {
        email: "bob@example.com".to_string(),
        name: "Bob".to_string(),
        age: 30,
    },
    UpsertUsersBatchRecord {
        email: "alice@example.com".to_string(),  // Duplicate - will update
        name: "Alice Updated".to_string(),
        age: 26,
    },
];

let results = upsert_users_batch(&client, users).await?;
// Returns 2 rows: Bob (new) and Alice (updated)
println!("Upserted {} users", results.len());
```

## CLI Features

### Commands

- **`generate`** - Generate Rust code from SQL query files

### CLI Options

#### Generate Command
- `-d, --database-url <URL>` - Database connection URL
- `-q, --queries-dir <DIR>` - Directory containing SQL query files
- `-o, --output <FILE>` - Custom output file path
- `-m, --module <NAME>` - Module name for generated code
- `--dry-run` - Preview generated code without writing files


## Examples

The `example-app/` directory contains:

- `queries/` - SQL files with query definitions organized by module
- `migrations/` - Database schema migrations for testing

## Workspace Commands

```bash
# Build everything
cargo build

# Test the library
cargo test -p automodel-lib

# Run the CLI tool
cargo run -p automodel-cli -- [args...]

# Run the example app
cargo run -p example-app

# Check specific package
cargo check -p automodel-lib
cargo check -p automodel-cli
```

## Error Handling and Custom Error Types

AutoModel provides sophisticated error handling with automatic constraint extraction and type-safe error types. Different types of queries return different error types based on whether they can violate database constraints.

### Error Type Overview

AutoModel generates two types of error enums:

1. **`ErrorReadOnly`** - For SELECT queries that cannot violate constraints
2. **`Error<C>`** - For mutation queries (INSERT, UPDATE, DELETE) with constraint tracking

### ErrorReadOnly - For Read-Only Queries

All SELECT queries return `ErrorReadOnly`, a simple error enum without constraint violation variants:

**Generated Code:**
```rust
#[derive(Debug)]
pub enum ErrorReadOnly {
    Database(sqlx::Error),
    RowNotFound,
}

impl From<sqlx::Error> for ErrorReadOnly {
    fn from(err: sqlx::Error) -> Self {
        ErrorReadOnly::Database(err)
    }
}
```

**Example Usage:**

`queries/users/get_user_by_id.sql`:
```sql
-- @automodel
--    expect: exactly_one
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

**Generated function:**
```rust
pub async fn get_user_by_id(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: i32
) -> Result<GetUserByIdItem, super::ErrorReadOnly>  // Returns ErrorReadOnly
```

### Error<C> - For Mutation Queries

Mutation queries (INSERT, UPDATE, DELETE) return `Error<C>` where `C` is a query-specific constraint enum. This provides type-safe handling of constraint violations.

### Automatic Constraint Extraction

AutoModel automatically extracts all constraints from your PostgreSQL database for each table referenced in mutation queries. This happens at build time by querying the PostgreSQL system catalogs.

**Extracted Constraint Information:**
- **Unique constraints** - Including primary keys and unique indexes
- **Foreign key constraints** - With referenced table and column information
- **Check constraints** - With constraint expression
- **NOT NULL constraints** - For columns that cannot be null
- **Domain check constraints** - CHECK constraints from domain types used by table columns

**Example:**
For a users table with:
```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    age INT CHECK (age >= 0),
    organization_id INT REFERENCES organizations(id)
);
```

AutoModel generates:
```rust
#[derive(Debug)]
pub enum InsertUserConstraints {
    UsersPkey,                    // PRIMARY KEY constraint
    UsersEmailKey,                // UNIQUE constraint on email
    UsersAgeCheck,                // CHECK constraint on age
    UsersOrganizationIdFkey,      // FOREIGN KEY to organizations
    UsersIdNotNull,               // NOT NULL constraint on id
    UsersEmailNotNull,            // NOT NULL constraint on email
}

impl TryFrom<ErrorConstraintInfo> for InsertUserConstraints {
    type Error = ();
    
    fn try_from(info: ErrorConstraintInfo) -> Result<Self, Self::Error> {
        match info.constraint_name.as_str() {
            "users_pkey" => Ok(InsertUserConstraints::UsersPkey),
            "users_email_key" => Ok(InsertUserConstraints::UsersEmailKey),
            "users_age_check" => Ok(InsertUserConstraints::UsersAgeCheck),
            "users_organization_id_fkey" => Ok(InsertUserConstraints::UsersOrganizationIdFkey),
            "users_id_not_null" => Ok(InsertUserConstraints::UsersIdNotNull),
            "users_email_not_null" => Ok(InsertUserConstraints::UsersEmailNotNull),
            _ => Err(()),  // Unknown constraints return error instead of panicking
        }
    }
}
```

The generic `Error<C>` type handles constraint violations gracefully:
```rust
pub enum Error<C: TryFrom<ErrorConstraintInfo>> {
    /// Contains Some(C) when constraint is recognized, None for unknown constraints
    /// The ErrorConstraintInfo always contains the raw constraint details from PostgreSQL
    ConstraintViolation(Option<C>, ErrorConstraintInfo),
    RowNotFound,
    PoolTimeout,
    InternalError(String, sqlx::Error),
}
```

### Custom Error Type Names with `error_type`

By default, AutoModel generates error type names based on the query name (e.g., `InsertUserConstraints`). You can customize this using the `error_type` configuration option.

**Basic Usage:**

`queries/users/insert_user.sql`:
```sql
-- @automodel
--    error_type: "UserError"  # Custom name instead of InsertUserConstraints
-- @end

INSERT INTO users (email, name, age) 
VALUES (#{email}, #{name}, #{age}) 
RETURNING id
```

**Generated Code:**
```rust
#[derive(Debug)]
pub enum UserError {
    UsersPkey,
    UsersEmailKey,
    UsersAgeCheck,
    // ... other constraints
}

impl TryFrom<ErrorConstraintInfo> for UserError {
    type Error = ();
    fn try_from(info: ErrorConstraintInfo) -> Result<Self, Self::Error> {
        // ... conversion logic
    }
}

pub async fn insert_user(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    email: String,
    name: String,
    age: i32
) -> Result<i32, super::Error<UserError>>  // Uses custom UserError
```

### Error Type Reuse

Multiple queries that operate on the same table(s) can reuse the same error type. AutoModel validates at build time that the constraints match exactly.

**Example:**

`queries/users/insert_user.sql`:
```sql
-- @automodel
--    error_type: "UserError"  # First query generates the error type
-- @end

INSERT INTO users (email, name, age) 
VALUES (#{email}, #{name}, #{age}) 
RETURNING id
```

`queries/users/update_user_email.sql`:
```sql
-- @automodel
--    error_type: "UserError"  # Reuses UserError - constraints must match
-- @end

UPDATE users SET email = #{email} 
WHERE id = #{user_id} 
RETURNING id
```

`queries/users/upsert_user.sql`:
```sql
-- @automodel
--    error_type: "UserError"  # Reuses UserError
-- @end

INSERT INTO users (email, name, age) 
VALUES (#{email}, #{name}, #{age})
ON CONFLICT (email) 
DO UPDATE SET name = EXCLUDED.name, age = EXCLUDED.age
RETURNING id
```

**Build-Time Validation:**

AutoModel ensures that when you reuse an error type:
1. The referenced error type exists (defined by a previous query)
2. The constraints extracted for the current query exactly match the constraints in the reused type
3. Both queries reference the same table(s)

## Supported PostgreSQL Types

AutoModel supports a comprehensive set of PostgreSQL types with automatic mapping to Rust types. All types support `Option<T>` for nullable columns.

### Boolean & Numeric Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `BOOL` | `bool` |
| `CHAR` | `i8` |
| `INT2` (SMALLINT) | `i16` |
| `INT4` (INTEGER) | `i32` |
| `INT8` (BIGINT) | `i64` |
| `FLOAT4` (REAL) | `f32` |
| `FLOAT8` (DOUBLE PRECISION) | `f64` |
| `NUMERIC`, `DECIMAL` | `rust_decimal::Decimal` |
| `OID`, `REGPROC`, `XID`, `CID` | `u32` |
| `XID8` | `u64` |
| `TID` | `(u32, u32)` |

### String & Text Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `TEXT` | `String` |
| `VARCHAR` | `String` |
| `CHAR(n)`, `BPCHAR` | `String` |
| `NAME` | `String` |
| `XML` | `String` |

### Binary & Bit Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `BYTEA` | `Vec<u8>` |
| `BIT`, `BIT(n)` | `bit_vec::BitVec` |
| `VARBIT` | `bit_vec::BitVec` |

### Date & Time Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `DATE` | `chrono::NaiveDate` |
| `TIME` | `chrono::NaiveTime` |
| `TIMETZ` | `sqlx::postgres::types::PgTimeTz` |
| `TIMESTAMP` | `chrono::NaiveDateTime` |
| `TIMESTAMPTZ` | `chrono::DateTime<chrono::Utc>` |
| `INTERVAL` | `sqlx::postgres::types::PgInterval` |

### Range Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `INT4RANGE` | `sqlx::postgres::types::PgRange<i32>` |
| `INT8RANGE` | `sqlx::postgres::types::PgRange<i64>` |
| `NUMRANGE` | `sqlx::postgres::types::PgRange<rust_decimal::Decimal>` |
| `TSRANGE` | `sqlx::postgres::types::PgRange<chrono::NaiveDateTime>` |
| `TSTZRANGE` | `sqlx::postgres::types::PgRange<chrono::DateTime<chrono::Utc>>` |
| `DATERANGE` | `sqlx::postgres::types::PgRange<chrono::NaiveDate>` |

### Multirange Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `INT4MULTIRANGE` | `serde_json::Value` |
| `INT8MULTIRANGE` | `serde_json::Value` |
| `NUMMULTIRANGE` | `serde_json::Value` |
| `TSMULTIRANGE` | `serde_json::Value` |
| `TSTZMULTIRANGE` | `serde_json::Value` |
| `DATEMULTIRANGE` | `serde_json::Value` |

### Network & Address Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `INET` | `std::net::IpAddr` |
| `CIDR` | `std::net::IpAddr` |
| `MACADDR` | `mac_address::MacAddress` |

### Geometric Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `POINT` | `sqlx::postgres::types::PgPoint` |
| `LINE` | `sqlx::postgres::types::PgLine` |
| `LSEG` | `sqlx::postgres::types::PgLseg` |
| `BOX` | `sqlx::postgres::types::PgBox` |
| `PATH` | `sqlx::postgres::types::PgPath` |
| `POLYGON` | `sqlx::postgres::types::PgPolygon` |
| `CIRCLE` | `sqlx::postgres::types::PgCircle` |

### JSON & Special Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `JSON` | `serde_json::Value` |
| `JSONB` | `serde_json::Value` |
| `JSONPATH` | `String` |
| `UUID` | `uuid::Uuid` |

### Array Types

All types support PostgreSQL arrays with automatic mapping to `Vec<T>`:

| PostgreSQL Array Type | Rust Type |
|----------------------|-----------|
| `BOOL[]` | `Vec<bool>` |
| `INT2[]`, `INT4[]`, `INT8[]` | `Vec<i16>`, `Vec<i32>`, `Vec<i64>` |
| `FLOAT4[]`, `FLOAT8[]` | `Vec<f32>`, `Vec<f64>` |
| `TEXT[]`, `VARCHAR[]` | `Vec<String>` |
| `BYTEA[]` | `Vec<Vec<u8>>` |
| `UUID[]` | `Vec<uuid::Uuid>` |
| `DATE[]`, `TIMESTAMP[]`, `TIMESTAMPTZ[]` | `Vec<chrono::NaiveDate>`, `Vec<chrono::NaiveDateTime>`, `Vec<chrono::DateTime<chrono::Utc>>` |
| `INT4RANGE[]`, `DATERANGE[]`, etc. | `Vec<sqlx::postgres::types::PgRange<T>>` |
| And many more... | See type mapping table above |

### Full-Text Search & System Types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `TSQUERY` | `String` |
| `REGCONFIG`, `REGDICTIONARY`, `REGNAMESPACE`, `REGROLE`, `REGCOLLATION` | `u32` |
| `PG_LSN` | `u64` |
| `ACLITEM` | `String` |

### Custom Enum Types

PostgreSQL custom enums are automatically detected and mapped to generated Rust enums with proper encoding/decoding support. See the Configuration Options section for details on enum handling.

## Disabling Formatting of Generated Code

AutoModel emits a `// @generated` marker in the first few lines of every generated file. To prevent `rustfmt` from reformatting generated code, add this to your workspace `rustfmt.toml`:

```toml
format_generated_files = false
```

When this option is set, `rustfmt` skips any file that contains `@generated` in its first five lines. See the [rustfmt documentation](https://rust-lang.github.io/rustfmt/?version=v1.6.0&search=#format_generated_files) for details.

## Advanced Guides

- [Composite Types vs JSONB](doc/composite-types-vs-jsonb.md) — choosing between PostgreSQL composite types and JSONB columns, with side-by-side comparisons of insert/batch insert/select, backward & forward compatibility analysis, and best practices for schema evolution without downtime.

## Requirements

- PostgreSQL database (for actual code generation)
- Rust 1.70+
- tokio runtime

## License

MIT License - see LICENSE file for details.
