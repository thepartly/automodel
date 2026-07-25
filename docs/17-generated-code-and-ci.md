# Generated Code, Modules & CI

> Prerequisite: [Installation & Configuration](01-installation.md).

AutoModel generates real `.rs` files that you commit to git. This page covers how
those files are organized, how to keep `rustfmt` from reformatting them, and how
CI verifies they are up to date.

## Module organization

Generated functions are organized into modules based on directory structure:

```
queries/
├── users/              # → src/generated/users.rs
│   ├── get_user.sql
│   └── create_user.sql
├── posts/              # → src/generated/posts.rs
│   └── get_post.sql
└── admin/              # → src/generated/admin.rs
    └── health_check.sql
```

The file path `queries/{module}/{function}.sql` determines the module and function
name; both must be valid Rust identifiers.

Override the module name in metadata:

```sql
-- @automodel
--    module: custom_module  # Override the directory-based module name
-- @end
```

## Committing generated code

Generated `.rs` files are checked into git on purpose. This makes AutoModel
diff-friendly and reviewable:

- Pull request reviewers (human or AI) see exactly what changed — a renamed field,
  a new column, an added constraint — with nothing hidden inside macro expansion.
- Any tool in the ecosystem (IDE, AI assistant, `cargo doc`) can read the actual
  types without a database connection or special tooling.
- The [query-analysis warnings file](14-telemetry-and-analysis.md) is committed
  too, so performance regressions (e.g. a new sequential scan) show up in the
  diff.

## Disabling `rustfmt` for generated files

AutoModel emits a `// @generated` marker in the first few lines of every generated
file. To prevent `rustfmt` from reformatting generated code, add this to your
workspace `rustfmt.toml`:

```toml
format_generated_files = false
```

When set, `rustfmt` skips any file containing `@generated` in its first five
lines. See the
[rustfmt documentation](https://rust-lang.github.io/rustfmt/?version=v1.6.0&search=#format_generated_files)
for details.

## CI: verifying generated code is up to date

Because generated code is committed, CI does not need a database — it only needs to
confirm the committed files match what the current SQL and schema would produce.

The `build.rs` from [Installation & Configuration](01-installation.md) implements
this with the URL-provider closure:

```rust
|| {
    if std::env::var("CI").is_err() {
        // Local dev: generate using the live database.
        std::env::var("AUTOMODEL_DATABASE_URL").map_err(|_| {
            "AUTOMODEL_DATABASE_URL environment variable must be set for code generation".to_string()
        })
    } else {
        // CI: refuse to generate; fail if generated code is stale.
        Err("Detecting not up to date AutoModel generated code in CI environment".to_string())
    }
}
```

- **Locally**, developers set `AUTOMODEL_DATABASE_URL` and regeneration runs
  against the live database; changes are committed.
- **In CI**, generation is skipped. If the committed generated code is out of
  date, the build fails — a signal to regenerate and commit.

This keeps CI fully offline and fast while guaranteeing the checked-in code
reflects the current SQL.

---

← Previous: [Supported PostgreSQL Types](16-postgres-types.md) · [Guide Index](README.md) · Next: [Metadata Block Reference →](metadata-reference.md)
