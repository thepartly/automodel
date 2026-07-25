# Upsert (INSERT … ON CONFLICT)

> Prerequisite: [Batch Insert with UNNEST](11-batch-insert-unnest.md).

PostgreSQL's `ON CONFLICT` clause handles conflicts when inserting data, enabling
"upsert" operations (insert if new, update if exists). AutoModel fully supports
this pattern for both single-row and batch operations.

## Understanding `EXCLUDED`

In the `DO UPDATE` clause, `EXCLUDED` is a special table reference containing the
row that **would have been inserted** if there had been no conflict:

```sql
INSERT INTO users (email, name, age)
VALUES ('alice@example.com', 'Alice', 25)
ON CONFLICT (email)
DO UPDATE SET
  name = EXCLUDED.name,      -- the name from the VALUES clause
  age = EXCLUDED.age,        -- the age from the VALUES clause
  updated_at = NOW()         -- set to current timestamp
```

- `EXCLUDED.name` / `EXCLUDED.age` refer to the values being inserted.
- `users.name` / `users.age` refer to the existing row's values.

You can mix both references:

```sql
-- Only update if the new age is greater than the existing age
DO UPDATE SET age = EXCLUDED.age WHERE users.age < EXCLUDED.age
```

## Single-row upsert

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

```rust
use crate::generated::users::upsert_user;
use crate::models::UserProfile;

// First call - creates a new user
let user = upsert_user(
    &client,
    "alice@example.com".to_string(),
    "Alice".to_string(),
    25,
    UserProfile { bio: "Developer".to_string() },
).await?;

// Second call with the same email - updates the existing user
let updated_user = upsert_user(
    &client,
    "alice@example.com".to_string(),
    "Alice Smith".to_string(),  // updated name
    26,                          // updated age
    UserProfile { bio: "Senior Developer".to_string() },
).await?;

assert_eq!(user.id, updated_user.id);  // same row
```

## Batch upsert with UNNEST

Combine [UNNEST](11-batch-insert-unnest.md) with `ON CONFLICT` for efficient batch
upserts:

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

```rust
use crate::generated::users::{upsert_users_batch, UpsertUsersBatchRecord};

let users = vec![
    UpsertUsersBatchRecord { email: "alice@example.com".to_string(), name: "Alice".to_string(), age: 25 },
    UpsertUsersBatchRecord { email: "bob@example.com".to_string(),   name: "Bob".to_string(),   age: 30 },
    UpsertUsersBatchRecord { email: "alice@example.com".to_string(), name: "Alice Updated".to_string(), age: 26 }, // duplicate → update
];

let results = upsert_users_batch(&client, users).await?;
// Returns 2 rows: Bob (new) and Alice (updated)
println!("Upserted {} users", results.len());
```

---

← Previous: [Batch Insert with UNNEST](11-batch-insert-unnest.md) · [Guide Index](README.md) · Next: [Error Handling & Constraints →](13-error-handling.md)
