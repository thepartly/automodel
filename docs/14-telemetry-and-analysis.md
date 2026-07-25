# Telemetry & Query Analysis

> Prerequisite: [Installation & Configuration](01-installation.md).

AutoModel instruments generated functions with tracing spans and analyzes every
query at build time using `EXPLAIN`. Both are configured globally in `build.rs`
and can be overridden per query.

## Telemetry

Generated functions can emit `tracing` spans. Configure the default level and SQL
inclusion in `build.rs`:

```rust
let defaults = automodel::DefaultsConfig {
    telemetry: automodel::DefaultsTelemetryConfig {
        level: automodel::TelemetryLevel::Debug,
        include_sql: true,
    },
    // ...
};
```

**Telemetry levels:**

- `none` — no instrumentation
- `info` — basic span creation with the function name
- `debug` — include the SQL query in the span (if `include_sql` is true)
- `trace` — include both the SQL query and parameters in the span

**Span fields:** instrumented functions follow OpenTelemetry-aligned field
names:

- `db.operation.name` — the query's source location (`module/name`), always set.
- `db.query.text` — the actual statement executed (with `$1, $2 …` placeholders),
  emitted only when `include_sql` is true. Bound parameter values are never
  interpolated. For static queries the statement is embedded at compile time; for
  conditional and choice-group queries — whose SQL is assembled at runtime — it is
  recorded onto the span after the builder finishes.

### Per-query telemetry

Override the global settings in a query's metadata block:

```sql
-- @automodel
--    telemetry:
--      level: trace              # none | info | debug | trace
--      include_params: [user_id, email]  # only these parameters are logged
--      include_sql: true         # include SQL in spans
-- @end

SELECT * FROM users WHERE id = #{user_id}
```

For privacy, restrict or disable parameter logging on sensitive queries:

```sql
-- @automodel
--    expect: exactly_one
--    telemetry:
--      include_params: []  # skip all parameters
--      include_sql: false
-- @end

DELETE FROM sessions WHERE created_at < #{cutoff_date}
```

## Query analysis (`ensure_indexes`)

During code generation, AutoModel runs `EXPLAIN` on every query. It:

- **Detects sequential scans** — queries that perform full table scans (often a
  missing index).
- **Flags multi-partition access** on partitioned tables.
- **Surfaces warnings during the build** and writes a warnings file committed to
  the repo, so reviewers (human or AI) catch performance problems before
  production.
- **Embeds the query plan** in each generated function's doc comments.

Enable or disable analysis globally:

```rust
let defaults = automodel::DefaultsConfig {
    ensure_indexes: true,
    // ...
};
```

### Per-query analysis

Override the global setting for a specific query — useful for DDL or one-off
queries where analysis is irrelevant:

```sql
-- @automodel
--    ensure_indexes: false   # disable analysis for this query
-- @end

CREATE TABLE IF NOT EXISTS sessions (
  id UUID PRIMARY KEY,
  created_at TIMESTAMPTZ DEFAULT NOW()
)
```

```sql
-- @automodel
--    ensure_indexes: true    # enable analysis for this query
-- @end

SELECT * FROM users WHERE email = #{email}
```

Because warnings are committed alongside the generated code, they show up in pull
request diffs — see [Generated Code, Modules & CI](17-generated-code-and-ci.md).

---

← Previous: [Error Handling & Constraints](13-error-handling.md) · [Guide Index](README.md) · Next: [CLI & Workspace Commands →](15-cli-reference.md)
