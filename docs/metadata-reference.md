# Metadata Block Reference

> Complete reference for the `-- @automodel` block. For a gentle introduction see
> [Metadata Basics](03-metadata-basics.md).

Every query may begin with an optional metadata block written as SQL comments in
YAML:

```sql
-- @automodel
--    key: value
--    # ...
-- @end

SELECT ...
```

The whole block is optional; every key within it is optional and falls back to a
default. This page lists all keys in one place.

## Full example

```sql
-- @automodel
--    description: Retrieve a user by their ID  # Function documentation
--    module: custom_module    # Override directory-based module name
--    expect: exactly_one       # exactly_one | possible_one | at_least_one | multiple
--    types:                    # Custom type mappings
--      profile: "crate::models::UserProfile"                            # query param/output by name
--      public.positive_int: "std::num::NonZeroI32"                      # domain type alias override
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"  # composite type field
--    telemetry:                # Per-query telemetry settings
--      level: trace
--      include_params: [id, name]
--      include_sql: false
--    ensure_indexes: true      # Enable performance analysis
--    multiunzip: false         # Enable for UNNEST-based batch inserts
--    conditions_type: false    # Use old/new struct for conditional queries
--    parameters_type: false    # Group all parameters into one struct
--    return_type: "UserInfo"   # Custom return type name
--    error_type: "UserError"   # Custom error enum name
--    conditions_type_derives:  # Additional derives for conditions struct
--      - serde::Serialize
--    parameters_type_derives:  # Additional derives for parameters struct
--      - serde::Deserialize
--    return_type_derives:      # Additional derives for return struct
--      - serde::Serialize
--      - PartialEq
--    error_type_derives:       # Additional derives for error enum
--      - serde::Serialize
-- @end

SELECT id, name FROM users WHERE id = #{id}
```

## Key reference

| Key | Type | Default | Description | Guide |
|-----|------|---------|-------------|-------|
| `description` | string | none | Doc comment on the generated function. | [Metadata Basics](03-metadata-basics.md) |
| `module` | string | directory name | Override the directory-based module name. | [Generated Code & Modules](17-generated-code-and-ci.md) |
| `expect` | enum | `multiple` | Result shape: `exactly_one`, `possible_one`, `at_least_one`, `multiple`. | [Expected Results](05-expected-results.md) |
| `types` | map | none | Custom PostgreSQL→Rust type mappings (1/2/3-segment keys). | [Custom Type Mappings](08-custom-types.md) |
| `telemetry` | map | global defaults | Per-query tracing (`level`, `include_params`, `include_sql`). | [Telemetry & Analysis](14-telemetry-and-analysis.md) |
| `ensure_indexes` | bool | global default | Enable/disable `EXPLAIN` analysis for this query. | [Telemetry & Analysis](14-telemetry-and-analysis.md) |
| `multiunzip` | bool | `false` | Generate a record struct for UNNEST batch inserts. | [Batch Insert with UNNEST](11-batch-insert-unnest.md) |
| `parameters_type` | bool \| string | `false` | Group parameters into a struct (`true`) or reuse a named struct. | [Struct Config & Reuse](10-struct-config-and-reuse.md) |
| `conditions_type` | bool \| string | `false` | Diff-based conditional parameters via old/new struct. | [Struct Config & Reuse](10-struct-config-and-reuse.md) |
| `return_type` | string | `{QueryName}Item` | Custom name for the return struct (enables reuse). | [Struct Config & Reuse](10-struct-config-and-reuse.md) |
| `error_type` | string | `{QueryName}Constraints` | Custom name for the constraint enum (mutations only). | [Error Handling](13-error-handling.md) |
| `conditions_type_derives` | list | none | Extra derives for the conditions struct. | [Struct Config & Reuse](10-struct-config-and-reuse.md#custom-derive-traits) |
| `parameters_type_derives` | list | none | Extra derives for the parameters struct. | [Struct Config & Reuse](10-struct-config-and-reuse.md#custom-derive-traits) |
| `return_type_derives` | list | none | Extra derives for the return struct. | [Struct Config & Reuse](10-struct-config-and-reuse.md#custom-derive-traits) |
| `error_type_derives` | list | none | Extra derives for the error enum. | [Struct Config & Reuse](10-struct-config-and-reuse.md#custom-derive-traits) |

## `telemetry` sub-keys

| Sub-key | Type | Description |
|---------|------|-------------|
| `level` | enum | `none` \| `info` \| `debug` \| `trace` |
| `include_params` | list | Parameter names to log (empty list logs none) |
| `include_sql` | bool | Include the SQL query in the span |

## `types` key formats

| Key format | Segments | Purpose | Example |
|-----------|----------|---------|---------|
| `field_name` | 1 | Map parameter/column by name | `profile: "UserProfile"` |
| `schema.domain` | 2 | Override domain type alias | `public.positive_int: "NonZeroI32"` |
| `schema.type.field` | 3 | Map composite type field | `public.users.social_links: "Vec<Link>"` |

Type values may carry a binding suffix: `@json` (default, JSON serialization) or
`@native` (type implements `sqlx` traits). See
[Custom Type Mappings](08-custom-types.md).

## Parameter suffixes (in SQL, not metadata)

These appear on `#{param}` references in the query body, not in the metadata block:

| Suffix | Generated type | Use case |
|--------|----------------|----------|
| (none) | `T` | Required parameter |
| `?` | `Option<T>` | Optional / conditional-block parameter |
| `??` | `Option<Option<T>>` | Conditional block + nullable |
| `[?]` | `Vec<Option<T>>` | Array with nullable elements |
| `?[?]` | `Option<Vec<Option<T>>>` | Optional array with nullable elements |
| `??[?]` | `Option<Option<Vec<Option<T>>>>` | Conditional + nullable array with nullable elements |

Column non-null override uses `{column!}` or `"column!"` on output columns — see
[Non-Null Column Override](09-non-null-override.md).

## Global defaults (`build.rs`)

Defaults that apply to all queries are set in `DefaultsConfig` when calling
`AutoModel::generate()` — see [Installation & Configuration](01-installation.md):

| Field | Description | Guide |
|-------|-------------|-------|
| `telemetry.level` / `telemetry.include_sql` | Default tracing configuration | [Telemetry & Analysis](14-telemetry-and-analysis.md) |
| `ensure_indexes` | Default `EXPLAIN` analysis toggle | [Telemetry & Analysis](14-telemetry-and-analysis.md) |
| `derives.{return,parameters,conditions,error}_type` | Default derives per generated type | [Struct Config & Reuse](10-struct-config-and-reuse.md#custom-derive-traits) |
| `multiunzip_crate` | `Itertools` (≤12 params) or `ManyUnzip` (≤196) | [Batch Insert with UNNEST](11-batch-insert-unnest.md#multiunzip-crate-selection) |

---

← Previous: [Generated Code, Modules & CI](17-generated-code-and-ci.md) · [Guide Index](README.md)
