# Batch Insert with UNNEST

> Prerequisite: [Named Parameters](04-named-parameters.md),
> [Expected Results](05-expected-results.md).

AutoModel supports efficient batch inserts using PostgreSQL's `UNNEST`, which
expands multiple arrays into a set of rows in a single query — far more efficient
than inserting rows one at a time.

## Basic UNNEST pattern

`UNNEST` expands parallel arrays into rows:

```sql
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  ARRAY['Alice', 'Bob', 'Charlie'],
  ARRAY['alice@example.com', 'bob@example.com', 'charlie@example.com'],
  ARRAY[25, 30, 35]
)
RETURNING id, name, email, age, created_at;
```

With AutoModel, pass array parameters instead:

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

Key points:

- Use array parameters: `#{name}::text[]`, `#{email}::text[]`, etc.
- Include explicit type casts for proper type inference.
- Set `expect: multiple` to return a vector of results.
- Set `multiunzip: true` for the ergonomic batch-insert mode.

## The `multiunzip` option

**Without `multiunzip`**, you pass a separate array per column:

```rust
insert_users_batch(
    &client,
    vec!["Alice".to_string(), "Bob".to_string()],
    vec!["alice@example.com".to_string(), "bob@example.com".to_string()],
    vec![25, 30],
).await?;
```

**With `multiunzip: true`**, AutoModel generates a record struct and takes a
single `Vec`:

```rust
#[derive(Debug, Clone)]
pub struct InsertUsersBatchRecord {
    pub name: String,
    pub email: String,
    pub age: i32,
}

insert_users_batch(
    &client,
    vec![
        InsertUsersBatchRecord { name: "Alice".to_string(), email: "alice@example.com".to_string(), age: 25 },
        InsertUsersBatchRecord { name: "Bob".to_string(),   email: "bob@example.com".to_string(),   age: 30 },
    ],
).await?;
```

## How `multiunzip` works

When `multiunzip: true` is enabled, AutoModel:

1. Generates an input record struct with fields matching your parameters.
2. Uses `itertools::multiunzip()` to transform `Vec<Record>` into a tuple of
   arrays `(Vec<name>, Vec<email>, Vec<age>)`.
3. Binds each array to the corresponding SQL parameter.

```rust
pub async fn insert_users_batch(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    items: Vec<InsertUsersBatchRecord>,   // single parameter instead of many arrays
) -> Result<Vec<InsertUsersBatchItem>, super::Error<InsertUsersBatchConstraints>>
```

```rust
use itertools::Itertools;

let (name, email, age): (Vec<_>, Vec<_>, Vec<_>) =
    items.into_iter().map(|item| (item.name, item.email, item.age)).multiunzip();

let query = query.bind(name);
let query = query.bind(email);
let query = query.bind(age);
```

## Nullable elements in batch inserts

Use the `??` suffix (or a `?` struct field with `multiunzip`) to allow array
elements to be `NULL`.

**Without multiunzip** — `Vec<Option<T>>`:

```sql
-- @automodel
--    expect: multiple
-- @end
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  #{names}::text[],
  #{emails}::text[],
  #{ages??}::int4[]  -- array where individual elements can be NULL
)
```

```rust
pub async fn insert_users(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    names: Vec<String>,
    emails: Vec<String>,
    ages: Vec<Option<i32>>,  // elements can be NULL
) -> Result<Vec<InsertUsersItem>, super::Error<InsertUsersConstraints>>
```

**With multiunzip** — an optional struct field:

```sql
-- @automodel
--    expect: multiple
--    multiunzip: true
-- @end
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  #{name}::text[],
  #{email}::text[],
  #{age?}::int4[]  -- ? in the struct field makes it optional
)
```

```rust
pub struct InsertUsersRecord {
    pub name: String,
    pub email: String,
    pub age: Option<i32>,  // unpacks to Vec<Option<i32>> via multiunzip
}
```

## Multiunzip crate selection

By default AutoModel uses `itertools::multiunzip()`, which supports up to **12**
parameters. For batch inserts with more columns, configure the `many-unzip` crate
(up to **196** parameters) in your `build.rs`:

```rust
let defaults = automodel::DefaultsConfig {
    // ... other config ...
    multiunzip_crate: automodel::MultiunzipCrate::ManyUnzip,
};
```

- `MultiunzipCrate::Itertools` (default) — up to 12 parameters.
- `MultiunzipCrate::ManyUnzip` — 13–196 parameters. Requires the dependency:

```toml
[dependencies]
many-unzip = "0.1"  # or latest version
```

The generated code uses the correct trait automatically (`use itertools::Itertools;`
or `use many_unzip::ManyUnzip;`); both provide `.multiunzip()`.

## Complete example

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

```rust
use crate::generated::posts::{insert_posts_batch, InsertPostsBatchRecord};

let posts = vec![
    InsertPostsBatchRecord { title: "First Post".to_string(),  content: "Content 1".to_string(), author_id: 1, published_at: chrono::Utc::now() },
    InsertPostsBatchRecord { title: "Second Post".to_string(), content: "Content 2".to_string(), author_id: 1, published_at: chrono::Utc::now() },
];

let inserted = insert_posts_batch(&client, posts).await?;
println!("Inserted {} posts", inserted.len());
```

## Array columns in batch inserts (`jsonb[]`, `text[]`, etc.)

`UNNEST` flattens multidimensional arrays, so you **cannot** pass `jsonb[][]` or
`text[][]` to insert into a `jsonb[]` or `text[]` column — the nested arrays would
be flattened into individual elements.

The workaround: pass each row's array value as a single `jsonb` (a JSON array),
then reconstruct the PostgreSQL array in SQL using `jsonb_array_elements`.

**Nullable array column** (`jsonb[] DEFAULT NULL`):

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

**Required array column** (`jsonb[] NOT NULL`):

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

How it works:

1. The generated code serializes each row's array value to a `jsonb` value (a JSON
   array like `[{"label":"rust"},{"label":"go"}]`) — transparent to the caller.
2. `UNNEST` on `jsonb[]` yields one `jsonb` scalar per row — no flattening.
3. `ARRAY(SELECT jsonb_array_elements(tags))` reconstructs the `jsonb[]`.
4. For nullable columns, the `CASE WHEN ... IS NULL THEN NULL` guard preserves SQL
   NULLs.

The `types:` annotation maps both the parameter and the output column to your
[custom Rust type](08-custom-types.md).

> **Why not `jsonb[][]`?** PostgreSQL requires uniform sub-array lengths in
> multidimensional arrays, and `UNNEST` flattens all dimensions — making
> `type[][]` unusable for variable-length per-row arrays.

**Plain `text[]` columns** (using `jsonb_array_elements_text`):

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

The pattern is identical: declare the parameter as `jsonb[]` so `UNNEST` receives
flat scalars; AutoModel serializes the Rust `Vec<String>` to a JSON array before
binding; `jsonb_array_elements_text()` extracts `text` values and `ARRAY(...)::text[]`
reconstructs the column.

## UNNEST with composite types

As an alternative to `multiunzip` (one array per column), use **PostgreSQL
composite types**: pass a single array of a composite (row) type. AutoModel
auto-generates the corresponding Rust struct with `Encode`, `Decode`, `Type`, and
`PgHasArrayType` implementations.

From the caller's perspective both approaches look the same — you pass a
`Vec<SomeStruct>` and get results back. The difference is under the hood:
`multiunzip` splits the struct into separate arrays, while composite types bind a
single typed array directly to PostgreSQL.

**When to prefer composite types over multiunzip:**

- Your input rows have nested structure (composites within composites).
- You don't want the `itertools` / `many-unzip` dependency.
- No `multiunzip: true` needed — the composite type is auto-detected.
- You want PostgreSQL's type system to validate input.

**Step 1 — define a composite type:**

```sql
CREATE TYPE public.user_with_links_input AS (
    name TEXT,
    email TEXT,
    social_links JSONB
);
```

**Step 2 — write the query using the composite array:**

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

AutoModel detects the composite type from the `::public.user_with_links_input[]`
cast and generates:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserWithLinksInput {
    pub name: Option<String>,
    pub email: Option<String>,
    pub social_links: Option<serde_json::Value>,
}

pub async fn insert_users_bulk_composite(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    items: Vec<UserWithLinksInput>,
) -> Result<Vec<InsertUsersBulkCompositeItem>, super::Error<InsertUsersBulkCompositeConstraints>>
```

**Step 3 — use in Rust:**

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
        social_links: None,
    },
];

let results = insert_users_bulk_composite(&pool, items).await?;
```

To map the composite fields to typed Rust structs instead of `serde_json::Value`,
see [Custom Type Mappings](08-custom-types.md#composite-type-field-mappings).

**Comparison: multiunzip vs composite type UNNEST**

| Aspect | `multiunzip: true` | Composite type |
|--------|-------------------|----------------|
| Rust caller API | `Vec<Record>` | `Vec<CompositeType>` (same feel) |
| SQL parameter style | Separate arrays: `#{name}::text[], #{email}::text[]` | Single array: `#{items}::composite_type[]` |
| Under the hood | Struct split into arrays via `multiunzip()` | Array of composite bound directly to PG |
| Requires DDL | No (built-in types) | Yes (`CREATE TYPE`) |
| Metadata config | `multiunzip: true` | None (auto-detected) |
| Nested composites | Not supported | Supported |
| Dependencies | `itertools` or `many-unzip` | None |

Both approaches produce the same result — efficient bulk inserts via a single
`INSERT ... SELECT * FROM UNNEST(...)` statement.

---

← Previous: [Struct Configuration & Reuse](10-struct-config-and-reuse.md) · [Guide Index](README.md) · Next: [Upsert (INSERT … ON CONFLICT) →](12-upsert.md)
