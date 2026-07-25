# Named Parameters

> Prerequisite: [Metadata Basics](03-metadata-basics.md).

Parameters are written as `#{parameter_name}` directly in your SQL. AutoModel
turns them into typed function arguments, in the order they first appear.

```sql
SELECT * FROM users WHERE id = #{user_id} AND status = #{status}
```

generates a function taking `user_id` and `status` arguments with types inferred
from the database.

## Optional parameters (`?`)

Add a `?` suffix to make a parameter `Option<T>`. This is most useful with
[conditional blocks](06-conditional-queries.md), where `None` removes the block
entirely, but it also works in plain expressions:

```sql
SELECT * FROM posts
WHERE user_id = #{user_id}
  AND (#{category?} IS NULL OR category = #{category?})
```

## Optional + nullable parameters (`??`)

Use `??` in a conditional block when a parameter is both **optional** (controls
whether the block is included) and **nullable** (can set the column to `NULL`).
It generates `Option<Option<T>>`:

```sql
UPDATE users
SET updated_at = NOW()
  #[, age = #{age??}]
WHERE id = #{user_id}
RETURNING *
```

- `None` → skip the conditional block entirely (no change)
- `Some(None)` → include the block, set the value to `NULL`
- `Some(Some(35))` → include the block, set the value to `35`

## Array parameters with nullable elements (`[?]`)

Use `[?]` when an array parameter's individual elements can be `NULL`, producing
`Vec<Option<T>>`:

```sql
INSERT INTO users (name, email, age)
SELECT * FROM UNNEST(
  #{names}::text[],
  #{emails}::text[],
  #{ages[?]}::int4[]  -- Vec<Option<i32>>: array whose elements can be NULL
)
```

## Suffix reference

| Suffix | Generated type | Use case |
|--------|----------------|----------|
| (none) | `T` | Required parameter |
| `?` | `Option<T>` | Optional / conditional-block parameter |
| `??` | `Option<Option<T>>` | Conditional block + nullable (skip / set NULL / set value) |
| `[?]` | `Vec<Option<T>>` | Array with nullable elements |
| `?[?]` | `Option<Vec<Option<T>>>` | Optional array with nullable elements |
| `??[?]` | `Option<Option<Vec<Option<T>>>>` | Conditional + nullable array with nullable elements |

The suffixes are orthogonal and compose: the first `?` controls optionality, a
second `?` adds value nullability, and `[?]` adds element nullability.

> **Note:** A top-level `Option<>` in a [custom type mapping](08-custom-types.md)
> is banned — use these suffixes instead. If a custom mapping like
> `Vec<Option<T>>` already has nullable elements, the `[?]` suffix is a no-op
> (no double-wrapping).

---

← Previous: [Metadata Basics](03-metadata-basics.md) · [Guide Index](README.md) · Next: [Expected Results →](05-expected-results.md)
