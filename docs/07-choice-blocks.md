# Exclusive Choice Blocks (Case Syntax)

> Prerequisite: [Conditional Queries](06-conditional-queries.md).

Additive [conditional blocks](06-conditional-queries.md) (`#[ ... ]`) are
AND-combined, so several can apply at once. Sometimes, though, the caller must
pick **exactly one** of several mutually-exclusive branches (e.g. a sort mode, or
one of several keyset-pagination orderings). Marking each block with a selector
directive turns the group into a single generated Rust `enum` argument, so
illegal combinations become unrepresentable.

## Directive syntax

A choice block starts with a directive as its very first token:

```
#[#{selector=variant}  ...sql...]     -- required: arg is `selector: Enum`
#[#{selector=variant?} ...sql...]     -- optional: arg is `selector: Option<Enum>`
```

- `selector` is the shared group name (same across all branches) and becomes the
  function argument name.
- `variant` is the branch name and becomes an enum variant.
- The trailing marker is optional: no marker makes the choice required, `?` makes
  it optional (where `None` selects the base query with all blocks removed).

## Example — keyset pagination over four sort modes

```sql
-- @automodel
--    description: Keyset pagination with mutually-exclusive sort modes
--    expect: multiple
-- @end
SELECT id, name, email, updated_at
FROM users
WHERE 1 = 1
#[#{sort=ua_asc}   AND (updated_at, id) > (#{cursor_ts?}, #{cursor_id?}) ORDER BY updated_at ASC,  id ASC  LIMIT #{page_size?}]
#[#{sort=ua_desc}  AND (updated_at, id) < (#{cursor_ts?}, #{cursor_id?}) ORDER BY updated_at DESC, id DESC LIMIT #{page_size?}]
#[#{sort=name_asc}                                                        ORDER BY name ASC,       id ASC  LIMIT #{page_size?}]
#[#{sort=name_desc}                                                       ORDER BY name DESC,      id DESC LIMIT #{page_size?}]
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

## Parameter placement

- Every parameter referenced **inside a branch** becomes a **field on that enum
  variant** — even if the same parameter name appears in more than one branch
  (like `page_size`), each variant gets its own field.
- Only ungrouped/additive parameters and other groups' selectors become top-level
  function arguments.

## Rules (enforced at build time with clear errors)

- A query may declare **multiple independent choice groups**; each distinct
  selector name becomes its own enum argument.
- A choice group may **coexist with additive `#[...]` blocks** in the same query;
  blocks without a directive keep their additive `Option<T>` behavior.
- A branch may contain **nested additive `#[...]` blocks**; their parameters
  become `Option<T>` fields, included only when `Some(...)`.
- A single branch may **span multiple blocks**: repeat its directive on each
  fragment that must switch together — e.g. an output column and its matching
  `JOIN` — and they are included or dropped as a unit.
- All branches of a group must use the **same** optionality (all required, or all `?`).
- Variant names within a group must be unique.
- A parameter may belong to **at most one** choice group.

## Mixing choice blocks with additive blocks

A choice group can share a query with ordinary additive `#[...]` blocks — for
example additive `WHERE` filters plus an enum-selected sort mode:

```sql
SELECT id, name, email FROM users
WHERE id >= #{min_id}
  #[AND name LIKE #{name_starts_with?}]   -- additive: Option<String> argument
  #[AND age >= #{age_from?}]              -- additive: Option<i32> argument
#[#{sort=unsorted}  LIMIT #{limit?}]
#[#{sort=name_asc}  ORDER BY name ASC,  id ASC  LIMIT #{limit?}]
#[#{sort=name_desc} ORDER BY name DESC, id DESC LIMIT #{limit?}]
```

The additive filters stay optional and combine freely, while `sort` is a required
enum picking exactly one ordering. Because `limit` is referenced inside every
branch, each variant carries its own `limit` field:

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

A branch may carry **no** parameters at all (e.g. `#[#{sort=unsorted} LIMIT 100]`
with a hardcoded limit), in which case it generates a plain unit variant.

## Multiple choice groups in one query

A query may declare several independent choice groups, each compiling to its own
enum argument and selected independently — e.g. an optional age-`range` group
alongside a required `sort` group:

```sql
SELECT id, name, email, age FROM users
WHERE email LIKE #{email_prefix}
  #[#{range=min?} AND age >= #{min_age?}]
  #[#{range=max?} AND age <= #{max_age?}]
#[#{sort=asc}  ORDER BY id ASC  LIMIT #{lim?}]
#[#{sort=desc} ORDER BY id DESC LIMIT #{lim?}]
```

`range` is optional (`?`, so `None` applies no age bound) and each branch carries
its own field, while `sort` is required and shares `lim` across both branches:

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

The only restriction is that a given parameter name may not be shared between two
different groups, since the generator numbers and binds each parameter once.

## Nested optional blocks inside a branch

A choice branch may contain **nested additive `#[...]` blocks**. The branch is
still selected by the enum, but each nested block is included only when its
parameter is `Some(...)`. Nested-block parameters become `Option<T>` fields on the
variant, while the branch's direct parameters stay plain fields.

This is ideal for **keyset pagination where the first page has no cursor** but
later pages do:

```sql
SELECT id, name, email, updated_at
FROM users
WHERE name LIKE #{name_prefix}
  #[#{sort=name_asc?}  #[AND (name, id) > (#{cur_val?}, #{cur_id?})] ORDER BY name ASC,  id ASC]
  #[#{sort=name_desc?} #[AND (name, id) < (#{cur_val?}, #{cur_id?})] ORDER BY name DESC, id DESC]
LIMIT #{lim};
```

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

A single branch may hold **several independent nested blocks**, and nested blocks
combine freely with mandatory direct parameters in the same branch:

```sql
SELECT id, name, email, age, is_active FROM users
WHERE name LIKE #{name_prefix}
  #[#{filter=by_active} AND is_active = #{want_active} #[AND age >= #{active_min_age?}]]
  #[#{filter=by_age}    AND age >= #{floor_age}        #[AND age <= #{ceil_age?}]]
LIMIT #{lim};
```

```rust
pub enum FilterUsersFilter {
    ByActive { want_active: bool, active_min_age: Option<i32> },  // direct + nested
    ByAge    { floor_age: i32,   ceil_age: Option<i32> },         // direct + nested
}
```

Here `want_active` / `floor_age` are always bound (plain fields), while
`active_min_age` / `ceil_age` gate their nested predicate and apply only when
`Some(...)`.

## Coordinated output: conditional columns, joins, and collections

The examples above vary *how a query filters and sorts*. The same selector
mechanism can vary **what a query returns** — including or omitting an output
column, a `JOIN`, or a whole nested entity — while keeping the **result shape
fixed** (every branch produces the same columns, so no per-branch row mapping is
needed).

The enabler is that one branch may **span several blocks**: repeat the *same*
`#{selector=variant}` directive on each fragment that must switch together.
AutoModel merges them into a single branch.

**Conditional column + coordinated join.** A caller flag decides whether each row
also carries a value from a self-join; when off, the `LEFT JOIN` is skipped and
the column comes back `NULL`:

```sql
SELECT
  u.id,
  u.name,
  #[#{referrer=on} r.age]#[#{referrer=off} NULL] AS referrer_age
FROM public.users u
#[#{referrer=on} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
ORDER BY u.id
```

Both `on` fragments (the `r.age` projection and the `LEFT JOIN`) belong to the
same branch, so they toggle together. `referrer_age` is always present in the
struct as `Option<i32>`:

```rust
pub enum UserOptionalReferrerReferrer { On, Off }

pub async fn user_optional_referrer(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    email_prefix: String,
    referrer: UserOptionalReferrerReferrer,
) -> Result<Vec<UserOptionalReferrerItem>, /* ... */> { /* ... */ }
```

> **Author the data branch first** (`on` before `off`): AutoModel infers output
> types from the first branch that yields columns. Project the off-branch as a
> literal (`NULL`, or a typed `NULL::some_type`) so the removed join's aliases
> vanish with it.

**Whole nested entity (composite, no JSON).** Project the entire joined row as a
native composite that maps to a generated struct — the field is `Option<T>`
because a row expression is inherently nullable:

```sql
SELECT u.id, u.name,
  #[#{referrer=on} r]#[#{referrer=off} NULL] AS referrer
FROM public.users u
#[#{referrer=on} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
-- referrer: Option<types::public::Users>
```

**Collection of children (no JSON aggregate).** Use `array_agg` over a child
table's implicit composite type; it decodes straight into `Vec<Struct>`:

```sql
SELECT u.id, u.name,
  #[#{posts=on} (SELECT array_agg(p ORDER BY p.id) FROM public.posts p WHERE p.author_id = u.id)]#[#{posts=off} NULL] AS posts
FROM public.users u
WHERE u.email LIKE #{email_prefix}
-- posts: Option<Vec<types::public::Posts>>
```

**Two selectors, one query.** Independent selectors compose freely — every On/Off
combination yields a valid, fixed-shape row. Identical `NULL` off-branches across
different groups do **not** collide; blocks are matched positionally, not by body
text:

```sql
SELECT u.id, u.name,
  #[#{referrer=on} r]#[#{referrer=off} NULL] AS referrer,
  #[#{posts=on} (SELECT array_agg(p ORDER BY p.id) FROM public.posts p WHERE p.author_id = u.id)]#[#{posts=off} NULL] AS posts
FROM public.users u
#[#{referrer=on} LEFT JOIN public.users r ON r.id = u.referrer_id]
WHERE u.email LIKE #{email_prefix}
-- args: referrer: ...Referrer, posts: ...Posts  (two independent enums)
```

---

← Previous: [Conditional Queries](06-conditional-queries.md) · [Guide Index](README.md) · Next: [Custom Type Mappings →](08-custom-types.md)
