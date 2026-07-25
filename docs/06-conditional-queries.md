# Conditional Queries

> Prerequisite: [Named Parameters](04-named-parameters.md).

Conditional queries dynamically include or exclude SQL clauses based on which
optional parameters are provided. This lets a single query adapt to many call
patterns without string concatenation.

## Conditional syntax

Wrap an optional SQL fragment in `#[...]`. The block is included only when its
`?` parameter is `Some`, and removed entirely when it's `None`.

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

- `#[AND name ILIKE #{name_pattern?}]` — included only if `name_pattern` is `Some`
- `#{name_pattern?}` — an [optional parameter](04-named-parameters.md) (note `?`)
- The whole block disappears if the parameter is `None`

## Runtime behavior

The same generated function produces different SQL depending on the arguments:

```rust
// Both parameters provided
search_users(executor, Some("%john%".to_string()), Some(25)).await?;
// SQL: "... WHERE 1=1 AND name ILIKE $1 AND age >= $2 ORDER BY created_at DESC"

// Only name pattern
search_users(executor, Some("%john%".to_string()), None).await?;
// SQL: "... WHERE 1=1 AND name ILIKE $1 ORDER BY created_at DESC"

// Only age
search_users(executor, None, Some(25)).await?;
// SQL: "... WHERE 1=1 AND age >= $1 ORDER BY created_at DESC"

// Neither
search_users(executor, None, None).await?;
// SQL: "... WHERE 1=1 ORDER BY created_at DESC"
```

## Mixing conditional and required parameters

Conditional and non-conditional parameters coexist freely:

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

```rust
pub async fn find_users_complex(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    name_pattern: String,                            // required
    min_age: Option<i32>,                            // optional
    since: Option<chrono::DateTime<chrono::Utc>>,    // optional
) -> Result<Vec<FindUsersComplexItem>, super::ErrorReadOnly>
```

## Best practice: `WHERE 1=1`

When every `WHERE` clause is conditional, start with `WHERE 1=1` so the SQL stays
valid no matter which blocks are removed:

```sql
SELECT * FROM users
WHERE 1=1
  #[AND name = #{name?}]
  #[AND age > #{min_age?}]
```

## Conditional `UPDATE`

The same syntax enables partial updates — only the provided fields are written:

`queries/users/update_user_fields.sql`:

```sql
-- @automodel
--    description: Update only the fields that are provided (not None)
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

```rust
// Update only the name
update_user_fields(executor, user_id, Some("Jane Doe".to_string()), None, None).await?;
// SQL: "UPDATE users SET updated_at = NOW(), name = $1 WHERE id = $2 RETURNING ..."

// Update name + email
update_user_fields(executor, user_id, Some("Jane".to_string()),
    Some("jane@example.com".to_string()), None).await?;
// SQL: "UPDATE users SET updated_at = NOW(), name = $1, email = $2 WHERE id = $3 RETURNING ..."
```

> **Note:** Always include at least one non-conditional `SET` clause (like
> `updated_at = NOW()`) so the `UPDATE` remains syntactically valid when every
> optional parameter is `None`.

To set nullable columns to `NULL` conditionally, use the `??` suffix — see
[Named Parameters](04-named-parameters.md#optional--nullable-parameters-).

## Diff-based conditionals

For the load → modify → save pattern, `conditions_type` compares an `old` and
`new` struct and includes only the clauses whose fields changed. See
[Struct Configuration & Reuse](10-struct-config-and-reuse.md#conditions_type-diff-based-conditional-parameters).

---

← Previous: [Expected Results](05-expected-results.md) · [Guide Index](README.md) · Next: [Exclusive Choice Blocks →](07-choice-blocks.md)
