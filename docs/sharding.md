# Application-Level Sharding

> Prerequisites: [Named Parameters](04-named-parameters.md),
> [Error Handling & Constraints](13-error-handling.md).

AutoModel can generate query functions that route to one of several databases
("shards") based on a **shard key** parameter. Sharding is implemented entirely
in application code: you provide the per-shard connection pools and a strategy
that maps a key to a shard index, and AutoModel wires every generated function to
resolve the right shard before it runs.

When sharding is enabled, each generated function takes a sharded executor as its
first argument instead of a plain `sqlx` executor, and uses the query's shard-key
parameter to pick the shard.

> The generated runtime uses `async fn` in traits, so sharding requires **Rust
> 1.75 or newer**. It depends only on `sqlx` and `tokio` — no extra runtime
> dependency on the `automodel` crate.

## Enabling sharding

Add a `sharding` block to `automodel.yml`. The **presence** of this block enables
sharding for every query in the project:

```yaml
queries_dir: queries
output_dir: src/generated

sharding:
  shard_key: user_id       # name of the parameter used to select a shard
  key_type: uuid::Uuid     # Rust type of the shard key (default: uuid::Uuid)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `shard_key` | string | — | The default parameter name used to route every query. Required to enable sharding. |
| `key_type` | string | `uuid::Uuid` | The Rust type of the shard key. Sets the default type parameter on the generated `ShardedExecutor` trait so call sites need no turbofish. |

Every query must expose a parameter with the configured `shard_key` name (or a
[per-query override](#per-query-shard-key-override)). The shard-key parameter must
be **required** and **non-nullable** — see [Validation rules](#validation-rules).

## What sharding changes in generated code

With sharding enabled, AutoModel emits a self-contained `sharding.rs` into your
output directory alongside the query modules, and changes every generated
function's first parameter from a `sqlx` executor to `&impl ShardedExecutor`:

```rust
// Without sharding:
pub async fn insert_account(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: uuid::Uuid,
    name: String,
    balance: i64,
) -> Result<InsertAccountItem, Error> { /* ... */ }

// With sharding (first argument routes on `user_id`):
pub async fn insert_account(
    sharded: &impl super::ShardedExecutor,
    user_id: uuid::Uuid,
    name: String,
    balance: i64,
) -> Result<InsertAccountItem, Error> { /* ... */ }
```

Inside the function, AutoModel calls `sharded.resolve(&user_id)` to obtain a
connection on the correct shard, then runs the statement against it.

## Implementing a shard strategy

Implement `ShardStrategy<K>` to map a key to a shard index in `0..shard_count`.
The method is `async`, so a strategy can consult a cache or a catalog database:

```rust
use my_app::generated::ShardStrategy;

#[derive(Clone, Copy, Default)]
pub struct ModuloStrategy;

impl ShardStrategy<uuid::Uuid> for ModuloStrategy {
    async fn shard_index(&self, key: &uuid::Uuid, shard_count: usize) -> usize {
        (key.as_u128() % shard_count as u128) as usize
    }
}
```

Real deployments typically use consistent hashing or a lookup table; the shape is
the same. The returned index must be strictly less than `shard_count`.

## Building a router

`PoolRouter<K, S>` holds one `sqlx::PgPool` per shard plus your strategy, and
implements `ShardedExecutor`. Because `resolve` takes `&self`, you normally share
a single `&router` (or an `Arc<PoolRouter>`) across tasks — no clone is needed.
`PoolRouter` is `Clone`, but cloning is O(shard_count) (it reallocates the pool
`Vec` and bumps each pool's `Arc`; it opens no new connections), so prefer `Arc`
for a cheap shared handle:

```rust
use my_app::generated::PoolRouter;

pub type AccountsRouter = PoolRouter<uuid::Uuid, ModuloStrategy>;

pub async fn build_router(
    shard_urls: &[String],
) -> Result<AccountsRouter, sqlx::Error> {
    let mut pools = Vec::with_capacity(shard_urls.len());
    for url in shard_urls {
        pools.push(sqlx::PgPool::connect(url).await?);
    }
    Ok(PoolRouter::new(pools, ModuloStrategy))
}
```

`pools[i]` is the pool for shard index `i`, so the ordering of the vector must
match the indices your `ShardStrategy` returns.

## Calling generated functions

Pass `&router` as the first argument. Routing happens automatically using the
shard-key parameter:

```rust
let router = build_router(&shard_urls).await?;
let user_id = uuid::Uuid::new_v4();

// Routed to the shard that owns `user_id`.
let account = generated::accounts::insert_account(
    &router, user_id, "Grace".to_string(), 50,
).await?;

let fetched = generated::accounts::get_account(&router, user_id).await?;
```

Because `resolve` takes `&self`, a shared `&PoolRouter` can be used concurrently
across tasks.

## Transactions pinned to a shard

Multi-statement transactions must stay on a single shard. `PoolRouter::begin(key)`
opens a transaction on the shard that owns `key` and returns a
`ShardedExecutorTransaction` **pinned** to that key. It also implements
`ShardedExecutor`, so it can be passed to generated functions:

```rust
let tx = router.begin(user_id).await?;

// Same shard key -> runs inside the transaction.
generated::accounts::insert_account(&tx, user_id, "Pinned".to_string(), 10).await?;
generated::accounts::update_balance(&tx, user_id, 999).await?;

tx.commit().await?;   // or tx.rollback().await?
```

If a query passed to a pinned transaction has a shard key that differs from the
pinned key, it fails with `ShardError::WrongShard` **before** touching the
database — this catches accidental cross-shard access inside a transaction:

```rust
let tx = router.begin(pinned).await?;
let other = uuid::Uuid::new_v4();

let result = generated::accounts::get_account(&tx, other).await;
assert!(matches!(
    result,
    Err(generated::ErrorReadOnly::Sharding(generated::ShardError::WrongShard)),
));
```

## Batch inserts across shards

A [batch insert](11-batch-insert-unnest.md) (`multiunzip: true`) receives a `Vec`
of record structs, each carrying its own shard key. Since a single `UNNEST`
statement runs on one connection, AutoModel enforces that **all rows resolve to
the same shard**:

- An empty batch is a no-op that never touches a shard and returns an empty
  result.
- If the batch's rows target more than one shard, the function returns
  `ShardError::InconsistentBatch` before issuing any SQL.

```rust
let rows = vec![
    generated::accounts::InsertAccountsBulkRecord { user_id: a, name: "a".into(), balance: 1 },
    generated::accounts::InsertAccountsBulkRecord { user_id: b, name: "b".into(), balance: 2 },
];

// `a` and `b` land on different shards:
let result = generated::accounts::insert_accounts_bulk(&router, rows).await;
assert!(matches!(
    result,
    Err(generated::Error::Sharding(generated::ShardError::InconsistentBatch)),
));
```

Group rows by shard on the caller side (one call per shard) to insert across
shards.

## Error handling

Shard-routing failures are surfaced through the generated error types as a
`Sharding` variant wrapping `ShardError`:

| `ShardError` variant | Meaning |
|----------------------|---------|
| `Acquire(sqlx::Error)` | Failed to acquire a connection or begin a transaction on the target shard. |
| `WrongShard` | A query's shard key did not match the shard its pinned transaction was opened for. |
| `InconsistentBatch` | A batch operation contained rows targeting more than one shard. |

Read-only queries surface it as `ErrorReadOnly::Sharding(..)`; mutations surface
it as `Error::Sharding(..)`. See [Error Handling & Constraints](13-error-handling.md)
for the rest of the error model.

## Per-query shard key override

A query can shard on a different parameter than the global `shard_key` by setting
`shard_key` in its metadata block. The per-query value takes precedence over the
global `sharding.shard_key`:

```sql
-- @automodel
--    description: Fetch an account, sharding on an explicitly named parameter.
--    expect: possible_one
--    shard_key: owner_id
-- @end

SELECT user_id, name, balance
FROM public.accounts
WHERE user_id = #{owner_id}
```

Here the function routes on `owner_id` instead of `user_id`.

## Validation rules

AutoModel validates the shard key at generation time and fails the build if any
of these do not hold:

- A parameter named by the (global or per-query) `shard_key` must exist in the
  query.
- The shard-key parameter must be **required** — not optional (`?`) or nullable
  (`??`).
- The shard-key parameter's type must match the configured `key_type`. For batch
  (`multiunzip`) queries the element type of the record field must match
  `key_type`.
- The shard key may not be a [choice-block](07-choice-blocks.md) selector or
  branch parameter, nor part of a `conditions_type` diff struct.

## Reference

The generated `sharding.rs` exposes these items (re-exported from your
`generated` module):

| Item | Purpose |
|------|---------|
| `ShardStrategy<K>` | Trait you implement to map a key to a shard index. |
| `ShardedExecutor<K>` | Trait implemented by routers and pinned transactions; generated functions accept `&impl ShardedExecutor`. |
| `PoolRouter<K, S>` | Router over per-shard pools; `new`, `pools`, `begin`. |
| `ShardedExecutorTransaction<'t, K>` | Transaction pinned to one shard; `commit`, `rollback`. |
| `ShardError` | Routing failure enum, surfaced as the `Sharding` error variant. |
