---
name: testing
description: "Run and test the example-app against a local PostgreSQL database. USE FOR: starting Postgres via Docker Compose, running SQL migrations, building with code generation, running the example app, verifying generated queries work end-to-end. DO NOT USE FOR: writing unit tests, CI/CD pipeline setup."
argument-hint: "Describe what you want to test or verify"
---

# Testing the Example App

## Prerequisites

- Docker (with Compose v2)
- Rust toolchain (cargo)
- `psql` CLI (from `postgresql-client` or equivalent)

## 1. Start PostgreSQL

From the workspace root:

```bash
docker compose up -d
```

This starts PostgreSQL 17 on **port 55432** with:
- User: `postgres`
- Password: `password`
- Database: `postgres`

Wait for healthy status:

```bash
docker compose ps  # should show "healthy"
```

## 2. Set the Database URL

```bash
# bash
export AUTOMODEL_DATABASE_URL="postgresql://postgres:password@localhost:55432/postgres"

# fish
set -x AUTOMODEL_DATABASE_URL "postgresql://postgres:password@localhost:55432/postgres"
```

Note, if you are developing from a container, database URL need to be asjusted to connect to the Postgres container directly

## 3. Run Migrations

```bash
bash scripts/migrate.sh
```

## 4. Build the Example App (Code Generation)

The build script in `example-app/build.rs` connects to the database at compile time to introspect queries and generate typed Rust code:

```bash
cargo build -p example-app
```

If `AUTOMODEL_DATABASE_URL` is not set, the build will fail with an error. This is expected — the code generator requires a live database connection.

## 5. Run the Example App

```bash
cargo run -p example-app
```

This executes all the test functions in `example-app/src/main.rs` against the database.

## 6. Run the Test Suite

```bash
cargo test -p example-app
```

Runs all integration tests in `example-app/tests/`.

## 7. Full Regeneration (Invalidate + Rebuild + Test)

To force a complete regeneration of all generated code and verify everything works:

```bash
# Invalidate generated code
rm example-app/src/generated/mod.rs

# Rebuild (triggers full code generation)
cargo build -p example-app

# Run the test suite
cargo test -p example-app
```

This is useful before releases or after changes to the codegen logic. AutoModel detects the missing `mod.rs` and regenerates all files.

## 8. Tear Down

Stop and remove the container (data preserved in volume):

```bash
docker compose down
```

To also wipe the database volume:

```bash
docker compose down -v
```

## Quick One-Liner (bash)

```bash
docker compose up -d && \
  sleep 2 && \
  export AUTOMODEL_DATABASE_URL="postgresql://postgres:password@localhost:55432/postgres" && \
  bash scripts/migrate.sh && \
  cargo run -p example-app
```

## Troubleshooting

- **Port conflict on 55432**: Change the host port in `docker-compose.yml` and update the URL accordingly.
- **Build fails with "AUTOMODEL_DATABASE_URL must be set"**: Ensure the env var is exported in the current shell.
- **Migration errors "already exists"**: The migrations are not idempotent for types/enums. Tear down with `docker compose down -v` and re-run.
- **Connection refused on localhost**: If running inside a Docker container (e.g. devcontainer), `localhost:55432` won't work. Connect your container to the Postgres network and use the container hostname instead:
  ```bash
  docker network connect automodel_default <your-container-name>
  export AUTOMODEL_DATABASE_URL="postgresql://postgres:password@automodel-testdb-1:5432/postgres"
  ```
