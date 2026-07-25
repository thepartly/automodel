# AutoModel Guide

A progressive, feature-by-feature guide to AutoModel — the SQL-first reverse ORM
for Rust. Start at the top and work down; each page builds on the previous one.

## Getting started

1. [Installation & Configuration](01-installation.md) — add AutoModel to your
   project, write `build.rs`, and understand the project layout.
2. [Getting Started: Your First Query](02-getting-started.md) — write a plain
   SQL file (no metadata) and call the generated Rust function.
3. [Metadata Basics](03-metadata-basics.md) — introduce the `-- @automodel`
   block and what it controls.

## Core query features

4. [Named Parameters](04-named-parameters.md) — `#{param}` binding and the
   optional / nullable / array suffixes.
5. [Expected Results](05-expected-results.md) — `expect:` and the shape of what
   a query returns.
6. [Conditional Queries](06-conditional-queries.md) — additive `#[...]` blocks
   and conditional `UPDATE`.
7. [Exclusive Choice Blocks](07-choice-blocks.md) — enum-selected, mutually
   exclusive branches (sort modes, keyset pagination, conditional joins).

## Types & structs

8. [Custom Type Mappings](08-custom-types.md) — override PostgreSQL→Rust
   mappings, JSON vs native, composite-type fields, and domain types.
9. [Non-Null Column Override](09-non-null-override.md) — force non-nullable
   types for expression columns.
10. [Struct Configuration & Reuse](10-struct-config-and-reuse.md) —
    `parameters_type`, `conditions_type`, `return_type`, and custom derives.

## Bulk & mutation patterns

11. [Batch Insert with UNNEST](11-batch-insert-unnest.md) — `multiunzip`,
    nullable elements, array columns, and composite-type UNNEST.
12. [Upsert (INSERT … ON CONFLICT)](12-upsert.md) — single-row and batch upserts.
13. [Error Handling & Constraints](13-error-handling.md) — `ErrorReadOnly`,
    `Error<C>`, constraint extraction, and custom `error_type`.

## Operations & reference

14. [Telemetry & Query Analysis](14-telemetry-and-analysis.md) — tracing spans,
    `EXPLAIN` warnings, and index checks.
15. [CLI & Workspace Commands](15-cli-reference.md) — the standalone CLI.
16. [Supported PostgreSQL Types](16-postgres-types.md) — the full type mapping
    reference.
17. [Generated Code, Modules & CI](17-generated-code-and-ci.md) — committing
    generated code, module organization, and CI up-to-date checks.

## Complete reference

- [Metadata Block Reference](metadata-reference.md) — every `-- @automodel` key
  in one place.

## Advanced guides

- [Application-Level Sharding](sharding.md) — route generated queries across
  multiple databases by a shard-key parameter, with pinned transactions and
  batch-consistency checks.
- [Composite Types vs JSONB](composite-types-vs-jsonb.md) — choosing between
  PostgreSQL composite types and JSONB columns, with schema-evolution best practices.

---

Next: [Installation & Configuration →](01-installation.md)
