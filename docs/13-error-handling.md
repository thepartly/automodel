# Error Handling & Constraints

> Prerequisite: [Expected Results](05-expected-results.md).

AutoModel provides type-safe error handling with automatic constraint extraction.
Different query kinds return different error types depending on whether they can
violate database constraints.

## Two error types

1. **`ErrorReadOnly`** — for `SELECT` queries that cannot violate constraints.
2. **`Error<C>`** — for mutation queries (`INSERT`, `UPDATE`, `DELETE`) where `C`
   is a query-specific constraint enum.

## `ErrorReadOnly` — read-only queries

All `SELECT` queries return `ErrorReadOnly`, a simple enum without constraint
variants:

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

`queries/users/get_user_by_id.sql`:

```sql
-- @automodel
--    expect: exactly_one
-- @end

SELECT id, name, email FROM users WHERE id = #{user_id}
```

```rust
pub async fn get_user_by_id(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: i32,
) -> Result<GetUserByIdItem, super::ErrorReadOnly>
```

## `Error<C>` — mutation queries

Mutation queries return `Error<C>`, where `C` is a generated constraint enum. This
enables type-safe handling of constraint violations:

```rust
pub enum Error<C: TryFrom<ErrorConstraintInfo>> {
    /// Some(C) when the constraint is recognized, None for unknown constraints.
    /// ErrorConstraintInfo always carries the raw constraint details from PostgreSQL.
    ConstraintViolation(Option<C>, ErrorConstraintInfo),
    RowNotFound,
    PoolTimeout,
    InternalError(String, sqlx::Error),
}
```

## Automatic constraint extraction

At build time, AutoModel queries PostgreSQL's system catalogs and extracts all
constraints for each table referenced in a mutation query:

- **Unique constraints** — including primary keys and unique indexes
- **Foreign key constraints** — with referenced table and column
- **Check constraints** — with the constraint expression
- **NOT NULL constraints** — for columns that cannot be null
- **Domain check constraints** — `CHECK` constraints from
  [domain types](08-custom-types.md#domain-type-alias-mappings) used by columns

For a table like:

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
    UsersPkey,                    // PRIMARY KEY
    UsersEmailKey,                // UNIQUE on email
    UsersAgeCheck,                // CHECK on age
    UsersOrganizationIdFkey,      // FOREIGN KEY to organizations
    UsersIdNotNull,               // NOT NULL on id
    UsersEmailNotNull,            // NOT NULL on email
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
            _ => Err(()),  // unknown constraints return Err instead of panicking
        }
    }
}
```

## Custom error type names with `error_type`

By default the enum is named after the query (`InsertUserConstraints`). Override
it with `error_type`:

`queries/users/insert_user.sql`:

```sql
-- @automodel
--    error_type: "UserError"  # Custom name instead of InsertUserConstraints
-- @end

INSERT INTO users (email, name, age)
VALUES (#{email}, #{name}, #{age})
RETURNING id
```

```rust
#[derive(Debug)]
pub enum UserError {
    UsersPkey,
    UsersEmailKey,
    UsersAgeCheck,
    // ... other constraints
}

pub async fn insert_user(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    email: String,
    name: String,
    age: i32,
) -> Result<i32, super::Error<UserError>>
```

## Error type reuse

Multiple queries operating on the same table(s) can reuse the same error type.
AutoModel validates at build time that the constraints match exactly.

```sql
-- queries/users/insert_user.sql
-- @automodel
--    error_type: "UserError"  # First query generates the error type
-- @end
INSERT INTO users (email, name, age) VALUES (#{email}, #{name}, #{age}) RETURNING id
```

```sql
-- queries/users/update_user_email.sql
-- @automodel
--    error_type: "UserError"  # Reuses UserError - constraints must match
-- @end
UPDATE users SET email = #{email} WHERE id = #{user_id} RETURNING id
```

```sql
-- queries/users/upsert_user.sql
-- @automodel
--    error_type: "UserError"  # Reuses UserError
-- @end
INSERT INTO users (email, name, age) VALUES (#{email}, #{name}, #{age})
ON CONFLICT (email) DO UPDATE SET name = EXCLUDED.name, age = EXCLUDED.age
RETURNING id
```

**Build-time validation** ensures that when you reuse an error type:

1. The referenced error type exists (defined by a previous query).
2. The constraints extracted for the current query exactly match those in the
   reused type.
3. Both queries reference the same table(s).

Add extra derives to the error enum via `error_type_derives` — see
[Struct Configuration & Reuse](10-struct-config-and-reuse.md#custom-derive-traits).

---

← Previous: [Upsert (INSERT … ON CONFLICT)](12-upsert.md) · [Guide Index](README.md) · Next: [Telemetry & Query Analysis →](14-telemetry-and-analysis.md)
