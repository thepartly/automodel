# Expected Results

> Prerequisite: [Metadata Basics](03-metadata-basics.md).

The `expect` metadata key controls how a query is executed and what its generated
function returns.

```sql
-- @automodel
--    expect: exactly_one
-- @end

SELECT id, name, email FROM users WHERE id = #{id}
```

## The four modes

| `expect` value | Runtime call | Returns | Behavior |
|----------------|--------------|---------|----------|
| `exactly_one` | `fetch_one()` | `Result<T, Error>` | Fails if 0 or more than 1 row |
| `possible_one` | `fetch_optional()` | `Result<Option<T>, Error>` | 0 or 1 row |
| `at_least_one` | `fetch_all()` | `Result<Vec<T>, Error>` | Fails if 0 rows |
| `multiple` | `fetch_all()` | `Result<Vec<T>, Error>` | 0 or more rows |

`multiple` is the **default** when `expect` is omitted.

## Examples

```sql
-- @automodel
--    expect: exactly_one    -- one row required (e.g. lookup by primary key)
-- @end
SELECT id, name FROM users WHERE id = #{id}
```

```sql
-- @automodel
--    expect: possible_one   -- zero or one row (e.g. lookup by unique column)
-- @end
SELECT id, name FROM users WHERE email = #{email}
```

```sql
-- @automodel
--    expect: at_least_one   -- fail if the result set is empty
-- @end
SELECT id, name FROM users WHERE org_id = #{org_id}
```

```sql
-- @automodel
--    expect: multiple       -- any number of rows (the default)
-- @end
SELECT id, name FROM users ORDER BY name
```

## Return struct

For a multi-column `SELECT`, AutoModel generates a `{QueryName}Item` struct from
the output columns and wraps it according to the table above (`T`, `Option<T>`,
or `Vec<T>`). To rename that struct or share it across queries, see
[Struct Configuration & Reuse](10-struct-config-and-reuse.md). Single-column
queries return the scalar type directly (e.g. `RETURNING id` → `i32`).

The error half of the `Result` depends on whether the query mutates data — see
[Error Handling & Constraints](13-error-handling.md).

---

← Previous: [Named Parameters](04-named-parameters.md) · [Guide Index](README.md) · Next: [Conditional Queries →](06-conditional-queries.md)
