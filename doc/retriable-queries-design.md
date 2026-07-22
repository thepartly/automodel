# Retriable Queries with Retry Budget

## Problem

Network errors between the application and PostgreSQL are transient and retriable for read-only/idempotent queries. Naive application-level retry (e.g., "retry 3 times with backoff") is dangerous because during network instability, every in-flight request retries simultaneously, causing a retry storm that amplifies load and cascades failures.

## Solution: Budget-Based Retry

Queries marked as safely retriable get automatic retry on network errors, gated by a **shared retry budget** that caps retries to a configurable percentage of total requests (e.g., 2%). This means:

- Under normal conditions (rare transient errors): retries succeed transparently
- Under instability (many failures): the budget depletes quickly, most requests fail-fast without retry, preventing amplification

This is the same pattern used by service meshes (Envoy/Istio retry budgets).

---

## Design

### 1. Query Annotation

New YAML frontmatter field: `retriable`

```sql
-- @automodel
--    description: Find user by email
--    expect: possible_one
--    retriable: true
-- @end

SELECT id, name, email FROM users WHERE email = #{email}
```

**Values:**
- `true` — mark as retriable (changes signature to `&PgPool`)
- `false` or omitted — non-retriable (default, keeps `impl Executor` signature)

**Rationale:** Retriability changes the function signature (`&PgPool` instead of `impl Executor`), which means the query can no longer be called inside a transaction. This is a semantic contract change that must be an explicit choice by the developer — never auto-inferred. Even a read-only query might be intentionally used inside a transaction for consistent reads.

### 2. Global Configuration (`automodel.yml`)

```yaml
queries_dir: queries
output_dir: src/generated

retry:
  # Enable retry code generation (default: false)
  enabled: true
  # Budget: max ratio of retries to total requests (default: 0.02 = 2%)
  budget_ratio: 0.02
  # Time window for budget calculation in seconds (default: 10)
  budget_window_secs: 10
  # Min requests in window before budget applies (default: 100)
  # Prevents budget from blocking retries when traffic is very low
  budget_min_requests: 100
```

### 3. Runtime Component: `RetryBudget`

Generated as part of the module prelude (or as a standalone runtime crate/module). Shared across all queries via `Arc`.

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Sliding-window retry budget that limits retry ratio
pub struct RetryBudget {
    /// Rolling window of total requests (ring buffer of per-slot counts)
    requests: Box<[AtomicU64]>,
    /// Rolling window of retries performed
    retries: Box<[AtomicU64]>,
    /// Budget ratio (e.g., 0.02 for 2%)
    budget_ratio: f64,
    /// Minimum requests before budget kicks in
    min_requests: u64,
    /// Window duration
    window: std::time::Duration,
    /// Slot duration (window / num_slots)
    slot_duration: std::time::Duration,
    /// Start time for slot calculation
    start: Instant,
    /// Number of slots in the ring buffer
    num_slots: usize,
}

impl RetryBudget {
    pub fn new(budget_ratio: f64, window_secs: u64, min_requests: u64) -> Self { ... }

    /// Record a request (call on every query execution)
    pub fn record_request(&self) { ... }

    /// Check if a retry is allowed and record it if so
    /// Returns true if retry budget permits a retry
    pub fn check_and_record_retry(&self) -> bool { ... }

    fn current_slot(&self) -> usize { ... }
    fn total_requests_in_window(&self) -> u64 { ... }
    fn total_retries_in_window(&self) -> u64 { ... }
}
```

**Key properties:**
- Lock-free (atomics only) — no contention on the hot path
- Sliding window — adapts to traffic changes
- Min-requests threshold — allows retries freely during low traffic (where budget math is unstable)

### 4. Error Classification

Network errors eligible for retry (from `sqlx::Error`):

```rust
fn is_network_error(err: &sqlx::Error) -> bool {
    matches!(err,
        sqlx::Error::Io(_) |                    // TCP/connection errors
        sqlx::Error::PoolTimedOut |             // Pool exhaustion (transient)
        sqlx::Error::WorkerCrashed              // Connection worker died
    )
    || matches!(err, sqlx::Error::Protocol(msg) if is_transient_protocol_error(msg))
}
```

**NOT retriable:**
- `sqlx::Error::Database(_)` — server understood the query, returned an error
- `sqlx::Error::RowNotFound` — query logic issue
- `sqlx::Error::Encode/Decode` — serialization bugs
- `sqlx::Error::Configuration` — config bugs

### 5. Generated Code Changes

#### Without retry (current behavior, unchanged for non-retriable queries):
```rust
pub async fn insert_user(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    name: String,
    email: String,
) -> Result<InsertUserItem, super::Error<InsertUserConstraints>> {
    let query = sqlx::query(r"INSERT INTO users ...").bind(&name).bind(&email);
    let row = query.fetch_one(executor).await?;
    // ... extract fields
}
```

#### With retry (for retriable queries):

```rust
pub async fn find_user_by_email(
    pool: &sqlx::PgPool,
    retry_budget: &super::RetryBudget,
    email: &str,
) -> Result<Option<FindUserByEmailItem>, super::ErrorReadOnly> {
    retry_budget.record_request();

    // Query logic defined once in a closure, called twice if needed
    let execute = || async {
        let query = sqlx::query(r"SELECT ... WHERE email = $1").bind(email);
        query.fetch_optional(pool).await
    };

    let row = match execute().await {
        Ok(row) => row,
        Err(err) if super::is_network_error(&err) && retry_budget.check_and_record_retry() => {
            execute().await?
        }
        Err(err) => return Err(err.into()),
    };

    match row {
        Some(row) => { /* extract fields */ Ok(Some(...)) }
        None => Ok(None),
    }
}
```

**Key design decision — executor type changes for retriable queries:**

The current `impl sqlx::Executor<'_>` is consumed on use. For retry, we need to re-acquire a connection. Two options:

| Option | Signature | Tradeoff |
|--------|-----------|----------|
| **A. Pool reference** | `pool: &sqlx::PgPool, budget: &RetryBudget` | Clean, compile-time safety |
| **B. Pool + optional budget** | `executor: impl Executor, budget: Option<&RetryBudget>` | Flexible, but no compile-time safety |
| **C. Macro wrapper** | Keep current signature, provide `with_retry!` macro | No codegen change, user wraps calls |

**Recommended: Option A for retriable queries.** Rationale:
- **Correctness**: A retry on network error acquires a fresh connection from the pool and re-executes the query. If the original call was inside a transaction, the retry would execute *outside* that transaction — silently committing data independently, violating atomicity. The `&PgPool` signature makes it a **compile error** to accidentally pass a transaction, preventing this silent data corruption.
- Retriable queries are read-only/idempotent — they should not run inside transactions in normal usage.
- If the user genuinely needs the query inside a transaction, they use the non-retriable variant (same query with `impl Executor` signature, no retry). This can be achieved by either marking `retriable: false` or by generating both variants (see below).

**Dual-signature generation (optional future enhancement):**
For queries marked retriable, generate two functions:
- `find_user_by_email(pool, budget, ...)` — retriable, takes `&PgPool`
- `find_user_by_email_in_tx(executor, ...)` — non-retriable, takes `impl Executor`

This allows the same query to be used both standalone (with retry) and inside transactions (without retry), with the caller making an explicit choice.

### 6. Implementation Plan

#### Phase 1: Annotation & Detection
1. Add `retriable: Option<bool>` to `QueryDefinition`
2. Parse from YAML frontmatter in `sqlfile_parser.rs`
3. Add `retry` section to global config parsing
4. Resolve: `retriable: true` in annotation → generate retry variant with `&PgPool` signature
5. Store resolved `is_retriable: bool` in `QueryDefinitionRuntime`

#### Phase 2: Runtime Library
6. Implement `RetryBudget` struct (sliding window, atomics)
7. Implement `is_network_error()` classifier
8. Add to generated module prelude (or separate `automodel-runtime` crate)
9. Unit tests for budget behavior (depletion, refill, edge cases)

#### Phase 3: Code Generation
10. Modify `module_generator.rs` to emit retry-aware function bodies for retriable queries
11. Change executor parameter to `&PgPool` for retriable queries
12. Add `retry_budget: &RetryBudget` parameter
13. Generate retry logic (single retry on network error if budget allows)
14. Ensure telemetry spans capture retry attempts

#### Phase 4: Testing & Documentation
15. Add example queries with `retriable: true`
16. Integration test: simulate network error, verify retry behavior
17. Integration test: verify budget depletion stops retries
18. Document in README

---

## Open Questions

1. **Single retry vs. configurable max retries?**
   Single retry is simpler and sufficient — if the network is so broken that a single retry fails too, more retries won't help and will just waste budget. Recommendation: single retry only.

2. **Backoff between retry?**
   For a single retry on network error, immediate retry is fine (the connection is already dead, we're getting a fresh one from the pool). No backoff needed.

3. **Should `PoolTimedOut` be retriable?**
   Debatable. If the pool is exhausted, retrying immediately will likely fail again. Recommendation: make it configurable, default to NOT retriable (pool exhaustion is usually a capacity issue, not transient).

4. **Separate `automodel-runtime` crate?**
   Currently all generated code lives in the user's crate. The `RetryBudget` could be:
   - Generated inline (no extra dependency, but code duplication across projects)
   - Published as `automodel-runtime` crate (clean, versioned, testable)
   
   Recommendation: `automodel-runtime` crate — it's the natural home for runtime utilities as the project grows.

5. **Observability of retries?**
   Retries should emit a tracing event (e.g., `tracing::warn!("retrying query after network error")`) and the budget utilization should be observable (expose `current_ratio()` method for metrics).

6. **Transaction safety?**
   A retriable query retried on a fresh connection would execute **outside** the caller's transaction, silently violating atomicity. This is not just "retry can't work" — it's silent data corruption. Option A (`&PgPool` signature) makes this a compile-time error: you cannot pass a transaction to a retriable function. If the user needs the same query inside a transaction, they use the `_in_tx` variant (or mark `retriable: false`).
