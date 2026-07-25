# Non-Null Column Override

> Prerequisite: [Metadata Basics](03-metadata-basics.md).

By default, expression columns (computed values, function results, literals) are
generated as `Option<T>`, because PostgreSQL's prepared-statement metadata doesn't
report nullability for expressions — only for direct table columns with
`NOT NULL` constraints.

When you know an expression result can never be `NULL`, use the `!` suffix to
override this and generate a non-nullable type.

## Native syntax — `{column_name!}`

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

## sqlx-compatible syntax — `"column_name!"`

For easy migration from sqlx, the quoted-identifier syntax is also supported:

```sql
SELECT expires_at > now() AS "is_unexpired!" FROM sessions WHERE id = #{id}
```

## Mixing both syntaxes

```sql
SELECT
    id AS {user_id!},
    name AS {user_name!},
    created_at > now() - interval '1 year' AS {is_recent!},
    true AS "is_active!"
FROM users
```

Both syntaxes are rewritten to clean SQL at build time — the `!`, `{`, `}`, and
surrounding quotes are stripped from the runtime query sent to PostgreSQL. The
override only affects the generated Rust type (removing the `Option<>` wrapper);
it does not change query behavior.

---

← Previous: [Custom Type Mappings](08-custom-types.md) · [Guide Index](README.md) · Next: [Struct Configuration & Reuse →](10-struct-config-and-reuse.md)
