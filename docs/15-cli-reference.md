# CLI & Workspace Commands

> Prerequisite: [Installation & Configuration](01-installation.md).

AutoModel ships as a standalone CLI (`automodel-cli`) for generating code outside
of `build.rs`. This page also lists the workspace commands used when developing
AutoModel itself.

## CLI

The CLI reads its settings from an [`automodel.yml`](01-installation.md#2-add-an-automodelyml)
config file — the same file used by `build.rs` — so it reuses the same
`queries_dir`, `output_dir`, telemetry, derives, and other defaults. This means
the CLI produces exactly the same output as build-time generation.

### `generate`

Generate Rust code from SQL query files.

```bash
# Reads ./automodel.yml and connects using AUTOMODEL_DATABASE_URL
automodel generate

# Point at a specific config file and database URL
automodel generate --config path/to/automodel.yml --database-url postgresql://localhost/mydb

# Help
automodel --help
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --config <FILE>` | `automodel.yml` | Path to the `automodel.yml` config file |
| `-d, --database-url <URL>` | `AUTOMODEL_DATABASE_URL` env var | PostgreSQL connection URL (overrides the env var) |

The queries directory and output directory are **not** command-line flags — they
come from `queries_dir` and `output_dir` in the config file.

## Example app

The `example-app/` directory demonstrates build-time code generation:

- `queries/` — SQL files organized by module
- `migrations/` — database schema migrations for testing

## Workspace commands

```bash
# Build everything
cargo build

# Test the library
cargo test -p automodel-lib

# Run the CLI tool
cargo run -p automodel-cli -- [args...]

# Run the example app
cargo run -p example-app

# Check specific packages
cargo check -p automodel-lib
cargo check -p automodel-cli
```

---

← Previous: [Telemetry & Query Analysis](14-telemetry-and-analysis.md) · [Guide Index](README.md) · Next: [Supported PostgreSQL Types →](16-postgres-types.md)
