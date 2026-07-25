# Getting Started: Your First Query

> Prerequisite: [Installation & Configuration](01-installation.md).

AutoModel queries are plain `.sql` files. You don't need any special metadata to
get started — write SQL, build, and call the generated function.

## 1. Write a SQL file

Create `queries/users/get_user_by_id.sql`:

```sql
SELECT id, name, email, created_at
FROM users
WHERE id = #{id}
```

The only AutoModel-specific syntax here is `#{id}` — a
[named parameter](04-named-parameters.md). Everything else is ordinary
PostgreSQL. There is no metadata block at all; sensible defaults are used.

## 2. Build

```bash
export AUTOMODEL_DATABASE_URL=postgresql://localhost/mydb
cargo build
```

AutoModel connects to the database, prepares the statement, reads the result
types, and writes `src/generated/users.rs` with a `get_user_by_id` function and a
`GetUserByIdItem` struct matching the selected columns.

## 3. Call the generated function

```rust
mod generated;

use tokio_postgres::Client;

async fn example(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let user = generated::users::get_user_by_id(client, 1).await?;
    println!("{} <{}>", user.name, user.email);
    Ok(())
}
```

The function name comes from the file name (`get_user_by_id.sql`), and the module
comes from the directory (`users/`).

## What you get by default

With no metadata, AutoModel applies these defaults:

- **Parameters** become positional function arguments in the order they appear.
- **Return type** is `Vec<GetUserByIdItem>` for a multi-row `SELECT` (the default
  `expect: multiple`). To return a single row, see
  [Expected Results](05-expected-results.md).
- **Error type** is `ErrorReadOnly` for read-only queries. Mutations get a
  constraint-aware error type — see [Error Handling](13-error-handling.md).
- A struct named `{QueryName}Item` is generated from the output columns.

## A first insert

Create `queries/users/create_user.sql`:

```sql
INSERT INTO users (name, email)
VALUES (#{name}, #{email})
RETURNING id
```

```rust
let new_id = generated::users::create_user(
    client,
    "John".to_string(),
    "john@example.com".to_string(),
).await?;
```

That's the whole loop: write SQL → build → call a typed function. Everything from
here on is opt-in refinement via the metadata block, which we introduce next.

---

← Previous: [Installation & Configuration](01-installation.md) · [Guide Index](README.md) · Next: [Metadata Basics →](03-metadata-basics.md)
