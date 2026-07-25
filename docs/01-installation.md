# Installation & Configuration

This page gets AutoModel wired into your project. By the end you'll have a
`build.rs` that connects to PostgreSQL once at build time and writes typed Rust
functions into your source tree.

## Project structure

AutoModel generates real `.rs` files from `.sql` files:

```
queries/users/get_user.sql  →  src/generated/users.rs  (checked into git)
```

A typical project layout:

```
my-project/
├── queries/
│   └── users/
│       ├── get_user_by_id.sql
│       ├── create_user.sql
│       └── update_user_profile.sql
├── build.rs
├── Cargo.toml
└── src/
    ├── main.rs
    └── generated/        # written by AutoModel, committed to git
```

The file path determines the generated module and function name:
`queries/{module}/{function}.sql`. Both `{module}` and `{function}` must be
valid Rust identifiers. See [Generated Code, Modules & CI](17-generated-code-and-ci.md)
for details.

## 1. Add to your `Cargo.toml`

```toml
[dependencies]
automodel = "0.12"

[build-dependencies]
automodel = "0.12"
tokio = { version = "1.0", features = ["rt"] }
```

The `rt` feature on `tokio` is required so `build.rs` can run the async generation
step.

## 2. Add an `automodel.yml`

Keep project-wide defaults in an `automodel.yml` next to `Cargo.toml`. This is the
configuration the example app uses:

```yaml
queries_dir: queries
output_dir: src/generated

telemetry:
  level: debug
  include_sql: true

ensure_indexes: true

derives:
  return_type: [Clone]
  parameters_type: [Clone]
  conditions_type: [Clone]
  error_type: [Clone]

multiunzip_crate: itertools
```

Every field is optional and falls back to a default (`telemetry.level` defaults to
`none`, `ensure_indexes` to `false`, `derives` to empty, `multiunzip_crate` to
`itertools`). Each is covered in its relevant guide:

- `telemetry` → [Telemetry & Query Analysis](14-telemetry-and-analysis.md)
- `ensure_indexes` → [Telemetry & Query Analysis](14-telemetry-and-analysis.md)
- `derives` → [Struct Configuration & Reuse](10-struct-config-and-reuse.md)
- `multiunzip_crate` → [Batch Insert with UNNEST](11-batch-insert-unnest.md)

## 3. Create a `build.rs`

`build.rs` runs before your crate compiles. It loads the config, connects to the
database, extracts types from prepared statements, and writes `.rs` files. After
that, builds are fully offline. This is the exact `build.rs` used by the example
app:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = automodel::AutoModelConfig::from_file("automodel.yml")?;

    automodel::AutoModel::generate(
        || {
            if std::env::var("CI").is_err() {
                std::env::var("AUTOMODEL_DATABASE_URL").map_err(|_| {
                    "AUTOMODEL_DATABASE_URL environment variable must be set for code generation"
                        .to_string()
                })
            } else {
                Err(
                    "Detecting not up to date AutoModel generated code in CI environment"
                        .to_string(),
                )
            }
        },
        &config.queries_dir,
        &config.output_dir,
        config.defaults(),
    )
    .await
}
```

The closure that returns the database URL controls **when** generation runs. Here
generation is skipped in CI (`CI` env var set): CI instead verifies that the
committed generated code is already up to date. See
[Generated Code, Modules & CI](17-generated-code-and-ci.md).

## 4. Set the database URL

Point AutoModel at a database it can introspect at build time:

```bash
export AUTOMODEL_DATABASE_URL=postgresql://localhost/mydb
cargo build
```

The database only needs the schema (tables, types, constraints) in place — it is
used solely to prepare statements and read metadata.

## Generating without `build.rs` (CLI)

If you'd rather not run code generation as part of the build, AutoModel ships a
standalone CLI that reuses the **same** `queries/*.sql` files and the **same**
`automodel.yml` — no application build required. It loads the config with
`AutoModelConfig::from_file`, so it produces exactly the same output as the
`build.rs` above.

```bash
# Reads ./automodel.yml (queries_dir, output_dir, telemetry, derives, ...)
# and connects using AUTOMODEL_DATABASE_URL
automodel generate

# Or point at a specific config file and database URL explicitly
automodel generate --config automodel.yml --database-url postgresql://localhost/mydb
```

Use it for one-off regeneration, scripts, or CI. Because it skips compiling your
application, it is typically several orders of magnitude faster than a full
`build.rs`-driven build — ideal for tight edit-regenerate loops. See
[CLI & Workspace Commands](15-cli-reference.md) for the full reference.

## Requirements

- PostgreSQL database (for code generation only)
- Rust 1.70+
- tokio runtime

---

← Previous: [Guide Index](README.md) · Next: [Getting Started: Your First Query →](02-getting-started.md)
