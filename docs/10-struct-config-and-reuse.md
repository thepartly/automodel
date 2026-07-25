# Struct Configuration & Reuse

> Prerequisite: [Expected Results](05-expected-results.md),
> [Conditional Queries](06-conditional-queries.md).

AutoModel provides four options that customize how structs and error types are
generated and reused across queries: `parameters_type`, `conditions_type`,
`return_type`, and `error_type`. They eliminate duplication, improve type safety,
and create cleaner APIs. This page also covers **custom derives**.

## Overview

| Option | Purpose | Default | Accepts | Generates |
|--------|---------|---------|---------|-----------|
| `parameters_type` | Group query parameters into a struct | `false` | `true` or struct name | `{QueryName}Params` struct |
| `conditions_type` | Diff-based conditional parameters | `false` | `true` or struct name | `{QueryName}Params` struct with old/new comparison |
| `return_type` | Custom name for return type struct | auto | struct name or omit | Custom or `{QueryName}Item` struct |
| `error_type` | Custom name for constraint enum (mutations only) | auto | error type name or omit | Custom or `{QueryName}Constraints` enum |

Any generated struct or error type can be referenced by other queries. AutoModel
validates at build time that the types are compatible and constraints match
exactly. (`error_type` is covered in [Error Handling](13-error-handling.md).)

## `parameters_type`: structured parameters

Group all query parameters into a single struct instead of passing them
individually.

`queries/users/insert_user_structured.sql`:

```sql
-- @automodel
--    parameters_type: true  # Generates InsertUserStructuredParams
-- @end

INSERT INTO users (name, email, age)
VALUES (#{name}, #{email}, #{age})
RETURNING id
```

```rust
#[derive(Debug, Clone)]
pub struct InsertUserStructuredParams {
    pub name: String,
    pub email: String,
    pub age: i32,
}

pub async fn insert_user_structured(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    params: &InsertUserStructuredParams,
) -> Result<i32, super::Error<InsertUserStructuredConstraints>>
```

```rust
let params = InsertUserStructuredParams {
    name: "Alice".to_string(),
    email: "alice@example.com".to_string(),
    age: 30,
};
insert_user_structured(executor, &params).await?;
```

**Reuse** by specifying an existing struct name:

```sql
-- queries/users/get_user_by_id_and_email.sql
-- @automodel
--    parameters_type: true  # Generates GetUserByIdAndEmailParams
-- @end
SELECT id, name, email FROM users WHERE id = #{id} AND email = #{email}
```

```sql
-- queries/users/delete_user_by_id_and_email.sql
-- @automodel
--    parameters_type: "GetUserByIdAndEmailParams"  # Reuses existing struct
-- @end
DELETE FROM users WHERE id = #{id} AND email = #{email} RETURNING id
```

Only one struct definition is generated, shared by both functions.

## `conditions_type`: diff-based conditional parameters

For queries with [conditional SQL](06-conditional-queries.md) (`#[...]` blocks),
generate a struct and compare `old` vs `new` values to decide which clauses to
include. Works with any query type.

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

```rust
pub struct UpdateUserFieldsDiffParams {
    pub name: String,
    pub email: String,
}

pub async fn update_user_fields_diff(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    old: &UpdateUserFieldsDiffParams,
    new: &UpdateUserFieldsDiffParams,
    user_id: i32,
) -> Result<(), super::Error<UpdateUserFieldsDiffConstraints>>
```

```rust
let old = UpdateUserFieldsDiffParams {
    name: "Alice".to_string(),
    email: "alice@example.com".to_string(),
};
let new = UpdateUserFieldsDiffParams {
    name: "Alicia".to_string(),               // Changed
    email: "alice@example.com".to_string(),   // Same
};
update_user_fields_diff(executor, &old, &new, 42).await?;
// Only executes: UPDATE users SET updated_at = NOW(), name = $1 WHERE id = $2
```

**How it works:**

- The struct contains only conditional parameters (those ending with `?` or `??`).
- Non-conditional parameters remain individual function arguments.
- At runtime, the function compares `old.field != new.field`.
- Only clauses where the field differs are included.

**Nullable fields with `??`** — use `??` when a field is nullable (e.g. an `age`
column that allows `NULL`):

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
    pub name: String,          // ?  → non-nullable field
    pub age: Option<i32>,      // ?? → nullable field (can be set to NULL)
}
```

If `old.age != new.age`, the clause is included — and `new.age` being `None`
means "set to NULL".

**Reuse** works the same way — pass an existing struct name instead of `true`.

## `return_type`: custom return type names

Customize the name of the return struct (generated for multi-column `SELECT`s)
and enable reuse across queries.

`queries/users/get_user_summary.sql`:

```sql
-- @automodel
--    return_type: "UserSummary"  # Custom name instead of GetUserSummaryItem
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

```rust
#[derive(Debug, Clone)]
pub struct UserSummary {
    pub id: i32,
    pub name: String,
    pub email: String,
}

pub async fn get_user_summary(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: i32,
) -> Result<UserSummary, super::ErrorReadOnly>
```

Multiple queries returning the same columns can share the struct by using the same
`return_type` name — only one `UserSummary` is generated.

## Cross-struct reuse

You can reuse struct names across query kinds. AutoModel will:

1. **Auto-generate** the struct if it doesn't exist yet (from the first query that
   uses it).
2. **Reuse** it if it already exists (from a previous query in the same module).
3. **Validate** that fields match exactly when reusing.

```sql
-- queries/users/get_user_info.sql
-- @automodel
--    return_type: "UserInfo"  # First use: generates UserInfo from return columns
-- @end
SELECT id, name, email FROM users WHERE id = #{user_id}
```

```sql
-- queries/users/update_user_info.sql
-- @automodel
--    parameters_type: "UserInfo"  # Second use: reuses UserInfo for parameters
-- @end
UPDATE users SET name = #{name}, email = #{email} WHERE id = #{id}
```

```rust
let user = get_user_info(executor, 42).await?;
let updated = UserInfo { name: "New Name".to_string(), ..user };
update_user_info(executor, &updated).await?;
```

## Custom derive traits

Add derive traits to generated structs and enums using `*_derives` options. These
are combined with the global defaults from your `build.rs`.

**Global defaults** (in `build.rs`):

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

This adds `Clone` to all generated structs, alongside the always-present `Debug`.

**Per-query additional derives** append to the global defaults:

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

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct UserId {
    pub id: i32,
}
```

Available options: `conditions_type_derives`, `parameters_type_derives`,
`return_type_derives`, `error_type_derives`.

**Trait merging:** global defaults are applied first, per-query derives are
appended, duplicates are removed, and `Debug` is always included.

## Build-time validation

AutoModel validates struct field compatibility at build time:

1. **Auto-generation** — if a named struct doesn't exist, it is generated from the
   query.
2. **Field matching** — when reusing a struct, query parameters/columns must
   exactly match struct fields (names and types). No subset matching.
3. **Clear errors:**

```
Error: Query parameter 'age' not found in struct 'UserInfo'.
Available fields: id, name, email
```

```
Error: Type mismatch for parameter 'id' in struct 'UserInfo':
expected 'i64', but query requires 'i32'
```

## Struct definition sources

Structs can be generated from:

1. `parameters_type: true` → `{QueryName}Params` (input parameters)
2. `conditions_type: true` → `{QueryName}Params` (conditional input parameters)
3. `return_type: "Name"` → custom named struct (output columns)
4. Multi-column `SELECT` → `{QueryName}Item` (output columns, when `return_type`
   is not specified)

## When to use each option

- **`parameters_type`** — queries with 3+ parameters, building params from API
  input, reusing parameter sets, reducing signature complexity.
- **`conditions_type`** — conditional queries with state comparison,
  PATCH-style updates that modify only changed fields, avoiding many
  `Option<T>` parameters.
- **`return_type`** — multiple queries returning the same columns, domain-specific
  struct names, reusing return types as inputs for related queries.

> **Note:** `parameters_type` is ignored when `conditions_type` is enabled —
> diff-based queries already use structured parameters.

---

← Previous: [Non-Null Column Override](09-non-null-override.md) · [Guide Index](README.md) · Next: [Batch Insert with UNNEST →](11-batch-insert-unnest.md)
