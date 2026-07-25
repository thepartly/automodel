# AutoModel — SQL-first Reverse ORM for Rust, Built for the greater DX and for the AI Era

Database access in Rust typically falls into two camps: **ORMs** (Diesel, SeaORM)
and **compile-time checked SQL** (sqlx). Both have trade-offs that become sharply
worse when an AI assistant — or any automated tool — is working with your code,
and humans are exposed to a far more intense code-review cycle.

**AutoModel is different: you write plain SQL, and the tool generates real Rust
source files.**

```
queries/users/get_user.sql  →  src/generated/users.rs  (checked into git)
```

## Why AutoModel

1. **Human or AI can read everything.** Generated structs, function signatures,
   error enums, and type aliases — all corresponding to the actual database
   schema, including constraints exposed as structured Rust enums — are ordinary
   `.rs` files in your repo. An LLM can inspect them and produce correct calling
   code on the first try — no database connection or special tooling required.
2. **Plain SQL stays plain SQL.** Your queries are `.sql` files with full syntax
   highlighting. No query builder, no expression DSL. Any valid PostgreSQL query
   works — window functions, CTEs, recursive queries, lateral joins, aggregations,
   `UNNEST` batch inserts, partitioned tables, domain types, composite types,
   conditional clauses — all of SQL, with no restrictions.
3. **Build-time code generation, not compile-time magic.** `build.rs` connects to
   the database once, extracts types from prepared statements, and writes `.rs`
   files — seconds, not the minutes of a full compile. After that, builds are
   offline. CI can verify generated code is up-to-date without a live database.
4. **Diff-friendly and reviewable.** Because the generated code is committed, pull
   request reviewers (human or AI) see exactly what changed — nothing hidden
   inside macro expansion.
5. **Built-in query analytics.** During generation, AutoModel runs `EXPLAIN` on
   every query. Each generated function includes the query plan in its doc
   comments, and a committed warnings file flags sequential scans and
   multi-partition access — surfaced at build and review time.
6. **Feature-rich control over generated code.** Struct reuse and deduplication,
   diff-based conditional updates, custom struct naming, `multiunzip` batch
   inserts, strongly typed `json`/`jsonb`, full composite-type support — and much
   more.
7. **Less code to write, review, and test.** The glue between SQL and Rust —
   structs, parameter binding, error enums, conversions — is machine-generated.
   Adding a query is a single `.sql` file, and you get a typed Rust function on the
   next build.

The result: a workflow where SQL is the source of truth, types are real files,
every tool in the ecosystem — IDE, AI, CI, code review — can see the full picture,
and development moves faster because an entire layer of boilerplate is eliminated.

## Project structure

This is a Cargo workspace with three main components:

- **`automodel-lib/`** — the core library for generating typed functions from SQL
- **`automodel-cli/`** — command-line interface
- **`example-app/`** — an example application demonstrating build-time generation

## Quick start

Add AutoModel to `Cargo.toml`:

```toml
[dependencies]
automodel = "0.12"

[build-dependencies]
automodel = "0.12"
tokio = { version = "1.0", features = ["rt"] }
```

Write a plain SQL query in `queries/users/get_user_by_id.sql`:

```sql
SELECT id, name, email, created_at
FROM users
WHERE id = #{id}
```

Build (with a `build.rs` that calls `automodel::AutoModel::generate`), then call
the generated function:

```rust
let user = generated::users::get_user_by_id(client, 1).await?;
```

Full setup, including `build.rs`, is in
[Installation & Configuration](https://github.com/thepartly/automodel/blob/master/docs/01-installation.md).

## Documentation

The full documentation is a progressive, feature-by-feature guide in
[**`docs/`**](https://github.com/thepartly/automodel/blob/master/docs/README.md). Start there and work down:

**Getting started**
- [Installation & Configuration](https://github.com/thepartly/automodel/blob/master/docs/01-installation.md)
- [Getting Started: Your First Query](https://github.com/thepartly/automodel/blob/master/docs/02-getting-started.md)
- [Metadata Basics](https://github.com/thepartly/automodel/blob/master/docs/03-metadata-basics.md)

**Core query features**
- [Named Parameters](https://github.com/thepartly/automodel/blob/master/docs/04-named-parameters.md)
- [Expected Results](https://github.com/thepartly/automodel/blob/master/docs/05-expected-results.md)
- [Conditional Queries](https://github.com/thepartly/automodel/blob/master/docs/06-conditional-queries.md)
- [Exclusive Choice Blocks](https://github.com/thepartly/automodel/blob/master/docs/07-choice-blocks.md)

**Types & structs**
- [Custom Type Mappings](https://github.com/thepartly/automodel/blob/master/docs/08-custom-types.md)
- [Non-Null Column Override](https://github.com/thepartly/automodel/blob/master/docs/09-non-null-override.md)
- [Struct Configuration & Reuse](https://github.com/thepartly/automodel/blob/master/docs/10-struct-config-and-reuse.md)

**Bulk & mutation patterns**
- [Batch Insert with UNNEST](https://github.com/thepartly/automodel/blob/master/docs/11-batch-insert-unnest.md)
- [Upsert (INSERT … ON CONFLICT)](https://github.com/thepartly/automodel/blob/master/docs/12-upsert.md)
- [Error Handling & Constraints](https://github.com/thepartly/automodel/blob/master/docs/13-error-handling.md)

**Operations & reference**
- [Telemetry & Query Analysis](https://github.com/thepartly/automodel/blob/master/docs/14-telemetry-and-analysis.md)
- [CLI & Workspace Commands](https://github.com/thepartly/automodel/blob/master/docs/15-cli-reference.md)
- [Supported PostgreSQL Types](https://github.com/thepartly/automodel/blob/master/docs/16-postgres-types.md)
- [Generated Code, Modules & CI](https://github.com/thepartly/automodel/blob/master/docs/17-generated-code-and-ci.md)
- [Metadata Block Reference](https://github.com/thepartly/automodel/blob/master/docs/metadata-reference.md)

## Advanced guides

- [Composite Types vs JSONB](https://github.com/thepartly/automodel/blob/master/docs/composite-types-vs-jsonb.md) —
  choosing between PostgreSQL composite types and JSONB columns, with side-by-side
  comparisons and schema-evolution best practices.

## Requirements

- PostgreSQL database (for code generation only)
- Rust 1.70+
- tokio runtime

## License

MIT License — see [LICENSE](https://github.com/thepartly/automodel/blob/master/LICENSE) for details.
