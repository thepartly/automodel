# Metadata Basics

> Prerequisite: [Getting Started](02-getting-started.md).

Everything in [Getting Started](02-getting-started.md) worked with zero
configuration. When you want to control the generated code — how many rows a
query returns, custom type mappings, struct names, telemetry, and more — you add
an **optional metadata block** at the top of the `.sql` file.

## The `-- @automodel` block

Metadata lives in SQL comments, in YAML, between `-- @automodel` and `-- @end`:

```sql
-- @automodel
--    description: Retrieve a user by their ID
--    expect: exactly_one
-- @end

SELECT id, name, email, created_at
FROM users
WHERE id = #{id}
```

Rules:

- The block is **entirely optional**. Omit it and defaults apply (as in
  [Getting Started](02-getting-started.md)).
- Every line starts with `--` (it's a SQL comment, so the file stays valid SQL).
- The body is YAML. Indentation matters, just like normal YAML.
- It must appear **before** the query.

## A minimal block

An empty block is legal and behaves exactly like having no block — it's a handy
placeholder:

```sql
-- @automodel
-- @end

SELECT id, name FROM users WHERE id = #{id}
```

## What metadata controls

Each key unlocks a feature covered later in this guide:

| Key | Purpose | Guide |
|-----|---------|-------|
| `description` | Doc comment on the generated function | this page |
| `expect` | How many rows / the return shape | [Expected Results](05-expected-results.md) |
| `module` | Override the directory-based module name | [Generated Code & Modules](17-generated-code-and-ci.md) |
| `types` | Custom PostgreSQL→Rust type mappings | [Custom Type Mappings](08-custom-types.md) |
| `telemetry` | Per-query tracing configuration | [Telemetry & Analysis](14-telemetry-and-analysis.md) |
| `ensure_indexes` | Per-query `EXPLAIN` analysis | [Telemetry & Analysis](14-telemetry-and-analysis.md) |
| `multiunzip` | Batch-insert record struct | [Batch Insert with UNNEST](11-batch-insert-unnest.md) |
| `parameters_type` | Group parameters into a struct | [Struct Config & Reuse](10-struct-config-and-reuse.md) |
| `conditions_type` | Diff-based conditional parameters | [Struct Config & Reuse](10-struct-config-and-reuse.md) |
| `return_type` | Custom return struct name | [Struct Config & Reuse](10-struct-config-and-reuse.md) |
| `error_type` | Custom constraint error enum name | [Error Handling](13-error-handling.md) |
| `*_derives` | Extra derive traits on generated types | [Struct Config & Reuse](10-struct-config-and-reuse.md) |

For a single page listing **every** key with its type and default, see the
[Metadata Block Reference](metadata-reference.md).

## `description`

The simplest key. It becomes the doc comment on the generated function, so it
shows up in IDE hover and `cargo doc`:

```sql
-- @automodel
--    description: Retrieve a user by their ID
-- @end

SELECT id, name, email FROM users WHERE id = #{id}
```

With the concept established, the next pages introduce features one at a time,
starting with the parameter syntax you've already seen a little of.

---

← Previous: [Getting Started](02-getting-started.md) · [Guide Index](README.md) · Next: [Named Parameters →](04-named-parameters.md)
