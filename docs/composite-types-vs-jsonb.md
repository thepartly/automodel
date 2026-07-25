# Composite Types in AutoModel

This document compares the two approaches for modeling structured data within PostgreSQL columns when using AutoModel:

1. **JSONB columns** — schema-less JSON stored in a single column, mapped to Rust types via serde
2. **Composite types** — PostgreSQL `CREATE TYPE` with named, typed fields, mapped via `sqlx::Type`

It covers insert, batch insert, and select operations, then analyzes backward/forward compatibility during schema migrations.

---

## Overview

| | JSONB Column | Composite Type |
|---|---|---|
| **Schema enforcement** | None at DB level | Yes, types checked by PostgreSQL |
| **App level type enforcement** | Yes, binding to user defined type at codegen time | Yes, types generated of the PostgreSQL schema |
| **Indexing** | GIN index on JSONB for containment queries | B-tree on individual fields via `((col).field)`, no GIN |
| **Nesting** | Arbitrary depth | Composites can nest other composites |
| **Schema evolution** | Flexible — add/remove fields freely | Rigid — requires `ALTER TYPE` DDL |
| **Query ergonomics** | Operators: `->`, `->>`, `@>`, `?` | Field access via `(col).field` or `UNNEST` |

---

## 1. JSONB Column Approach

Instead of `CREATE TYPE widget_metadata AS (...)`, store the same data as a JSONB column:

### Schema

```sql
CREATE TABLE IF NOT EXISTS public.widgets (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    weight FLOAT8,
    metadata JSONB,  -- instead of composite type
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);
```

### Rust model (user-defined)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetMetadata {
    pub color: String,
    pub version: i32,
}
```

The struct lives in your crate (e.g. `crate::models`) and is not generated — you own it.

### Insert

```sql
-- @automodel
--    types:
--      metadata: "crate::models::WidgetMetadata"
--      public.widgets.metadata: "crate::models::WidgetMetadata"
-- @end
INSERT INTO public.widgets (name, weight, metadata)
VALUES (#{name}, #{weight}, #{metadata})
RETURNING id, name, weight, metadata;
```

The generated function signature:

```rust
pub async fn insert_widget(
    pool: &sqlx::PgPool,
    name: String,
    weight: Option<f64>,
    metadata: crate::models::WidgetMetadata,
) -> Result<..., sqlx::Error>
```

### Batch Insert (multiunzip)

```sql
-- @automodel
--    multiunzip: true
--    types:
--      metadata: "crate::models::WidgetMetadata"
--      public.widgets.metadata: "crate::models::WidgetMetadata"
-- @end
INSERT INTO public.widgets (name, weight, metadata)
SELECT name, weight, metadata
FROM UNNEST(
        #{name}::text [],
        #{weight}::float8 [],
        #{metadata?}::jsonb []
    ) AS t(name, weight, metadata)
RETURNING id, name, weight, metadata;
```

Each scalar field is passed as a PostgreSQL array and fanned out with `UNNEST`. The JSONB field is passed as `jsonb[]` — an array of individual JSONB values, one per row. The `?` suffix makes `metadata` optional (nullable) per row.

### Select

```sql
-- @automodel
--    types:
--      public.widgets.metadata: "crate::models::WidgetMetadata"
-- @end
SELECT id, name, weight, metadata, created_at
FROM public.widgets
WHERE id = #{widget_id};
```

The type mapping on the output side tells AutoModel to decode the JSONB column into `WidgetMetadata`.

---

## 2. Composite Type Approach

The same `widget_metadata` data modeled as a PostgreSQL composite type:

### Schema

```sql
CREATE TYPE public.widget_metadata AS (
    color TEXT,
    version INT4
);

CREATE TABLE IF NOT EXISTS public.widgets (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    weight FLOAT8,
    metadata public.widget_metadata,  -- typed composite column
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Custom composite type for bulk input (decoupled from table structure)
CREATE TYPE public.widget_input AS (
    name TEXT,
    weight FLOAT8,
    metadata public.widget_metadata
);
```

### Generated Rust types

AutoModel generates `sqlx::Type` derives for composite types:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "widget_metadata")]
pub struct WidgetMetadata {
    pub color: Option<String>,
    pub version: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "widget_input")]
pub struct WidgetInput {
    pub name: String,
    pub weight: Option<f64>,
    pub metadata: Option<super::public::WidgetMetadata>,
}
```

Note: composite type fields are `Option` because PostgreSQL composite attributes are always nullable (there is no `NOT NULL` in `CREATE TYPE` DDL — it requires a [catalog hack](../example-app/migrations/009_make_widget_input_name_required.sql) via `pg_attribute`). With JSONB, your Rust struct fields are whatever you define them to be.

### Insert (single, via composite parameter)

```sql
INSERT INTO public.widgets (name, weight, metadata)
SELECT r.name, r.weight, r.metadata
FROM (SELECT (#{item}::public.widget_input).*) AS r
RETURNING id, name, weight, metadata
```

A single composite value is cast to the type, destructured, and inserted. The Rust call:

```rust
let item = WidgetInput {
    name: "Sprocket".to_string(),
    weight: Some(1.5),
    metadata: Some(WidgetMetadata { color: Some("red".into()), version: Some(1) }),
};
let result = widgets::insert_widget_single(pool, item).await?;
```

### Batch Insert (composite array UNNEST)

```sql
INSERT INTO public.widgets (name, weight, metadata)
SELECT r.name, r.weight, r.metadata
FROM UNNEST(#{items}::public.widget_input[]) AS r(name, weight, metadata)
RETURNING id, name, weight, metadata
```

An array of composite values is passed as a single parameter and fanned out with `UNNEST`. The Rust call:

```rust
let items = vec![
    WidgetInput { name: "A".into(), weight: Some(1.0), metadata: None },
    WidgetInput { name: "B".into(), weight: None, metadata: Some(WidgetMetadata { ... }) },
];
let results = widgets::insert_widgets_custom_type(pool, items).await?;
```

### Batch Insert (table type UNNEST)

```sql
-- Using the table's own composite type (all columns including id, created_at)
INSERT INTO public.widgets (name, weight, metadata)
SELECT r.name, r.weight, r.metadata
FROM UNNEST(#{items}::public.widgets[]) AS r(id, name, weight, metadata, created_at)
RETURNING id, name, weight, metadata
```

Note the column list in `AS r(...)` must include **all** table columns, even ones not selected. The custom composite type approach avoids this.

### Select

```sql
SELECT id, name, weight, metadata, created_at FROM public.widgets ORDER BY id
```

No special annotations needed — AutoModel resolves `metadata` to the `WidgetMetadata` composite type automatically from the database schema.

---

## 3. Side-by-Side Comparison

### Insert (single row)

| | JSONB | Composite |
|---|---|---|
| **SQL** | `VALUES (#{metadata})` — passed as a single `jsonb` | `SELECT (#{item}::widget_input).*` — cast + destructure |
| **Rust param** | `crate::models::WidgetMetadata` (auto-wrapped in `Json`) | `WidgetInput` (native `sqlx::Type`) |
| **Schema validation** | None — any JSON accepted at DB level | Full — PostgreSQL validates field names and types |

### Batch Insert

| | JSONB (multiunzip) | Composite (UNNEST) |
|---|---|---|
| **SQL pattern** | `UNNEST(#{name}::text[], #{weight}::float8[], #{metadata?}::jsonb[])` | `UNNEST(#{items}::widget_input[])` |
| **Parameter style** | One array per column (struct-of-arrays) | Single array of structs |
| **Rust input** | Generated `MultiunzipInput` struct with per-field `Vec`s | `Vec<WidgetInput>` |
| **Nullable per-row** | `?` suffix on individual fields | Fields nullable by composite type definition |
| **Schema coupling** | Low — SQL controls column mapping | Medium — composite type must match table columns |

### Select

| | JSONB | Composite |
|---|---|---|
| **Type annotation** | Required: `types:` block mapping column → Rust type | Automatic — resolved from DB schema |
| **Decode** | Via `sqlx::types::Json<T>` unwrap | Via `sqlx::Type` derive or manual `Decode` impl |
| **Nested types** | Transparent — serde handles any nesting | Each nested composite type needs its own `CREATE TYPE` |

---

## 4. Backward & Forward Compatibility

This is the critical section. When deploying schema changes, there is always a window where **old app + new schema** or **new app + old schema** coexist. The question: will inserts and selects crash or silently lose data?

### 4.1 Adding a Property

**Scenario:** A new field `label` is added to `widget_metadata`.

#### JSONB Column

| Situation | Behavior |
|---|---|
| **Old app → New schema** | Works. Old app writes `{"color":"red", "version":1}` without `label`. Old app reads rows with `label` — serde ignores unknown fields by default (`#[serde(deny_unknown_fields)]` would break this). |
| **New app → Old schema** | Works. New app writes `{"color":"red", "version":1, "label":"x"}`. PostgreSQL stores the extra field without complaint. Old app reading these rows ignores `label`. |
| **Migration** | No DDL needed. Just update the Rust struct and deploy. |

**Safe deployment order:** Deploy new app (writes extra field) → no migration needed.

#### Composite Type

| Situation | Behavior |
|---|---|
| **Old app → New schema** | **Breaks on INSERT.** After `ALTER TYPE widget_metadata ADD ATTRIBUTE label TEXT`, the old app's `sqlx::Type` encode sends `(color, version)` — PostgreSQL expects 3 fields and rejects it. SELECTs may also break depending on how the record is decoded. |
| **New app → Old schema** | **Breaks on INSERT.** New app encodes `(color, version, label)` — PostgreSQL expects 2 fields. |
| **Migration** | Requires `ALTER TYPE ... ADD ATTRIBUTE` DDL. Both app and schema must change simultaneously. |

**Safe deployment order:** This requires a coordinated deploy. There is no safe rolling-update window — the composite type's binary encoding is positional and field-count-sensitive.

#### Composite Type — Safe Migration Path

To avoid downtime with composite types, use the **versioned type** pattern:

```sql
-- 1. Create new version of the type
CREATE TYPE public.widget_metadata_v2 AS (
    color TEXT,
    version INT4,
    label TEXT
);

-- 2. Add a new column using the new type
ALTER TABLE public.widgets ADD COLUMN metadata_v2 public.widget_metadata_v2;

-- 3. Backfill
UPDATE public.widgets
SET metadata_v2 = ROW((metadata).color, (metadata).version, NULL)::widget_metadata_v2
WHERE metadata IS NOT NULL;

-- 4. After all apps migrated: drop old column and rename
ALTER TABLE public.widgets DROP COLUMN metadata;
ALTER TABLE public.widgets RENAME COLUMN metadata_v2 TO metadata;
```

This is significantly more work than the JSONB approach.

### 4.2 Removing a Property

**Scenario:** The `version` field is removed from `widget_metadata`.

#### JSONB Column

| Situation | Behavior |
|---|---|
| **Old app → New schema** | Works. Old app still writes `{"color":"red", "version":1}`. The extra `version` is stored harmlessly. |
| **New app → Old schema** | Works if `version` is `Option<i32>` in old app. New app writes `{"color":"red"}` without `version`. Old app deserializes — if `version` is required in old struct, **deserialize fails**. |
| **Migration** | No DDL required. |

**Safe deployment order:**
1. Make `version` optional in old app: `pub version: Option<i32>` — deploy.
2. Deploy new app that stops writing `version`.
3. Optionally clean up old data: `UPDATE widgets SET metadata = metadata - 'version'`.

The key insight: **make fields `Option<T>` before removing them** to ensure the old app can read rows written by the new app.

#### Composite Type

| Situation | Behavior |
|---|---|
| **Old app → New schema** | **Breaks.** After `ALTER TYPE widget_metadata DROP ATTRIBUTE version`, old app encodes `(color, version)` — PostgreSQL expects `(color)`. |
| **New app → Old schema** | **Breaks.** New app encodes `(color)` — PostgreSQL expects `(color, version)`. |
| **Migration** | Requires `ALTER TYPE ... DROP ATTRIBUTE`. Same coordinated deploy problem as adding a field. |

**Safe deployment order:** Same versioned-type approach as adding a field.

### 4.3 Compatibility Summary

| Change | JSONB — Zero-Downtime? | Composite — Zero-Downtime? |
|---|---|---|
| Add optional field | Yes — no migration needed | No — requires coordinated deploy or versioned type |
| Add required field | Yes (with default in serde) | No |
| Remove field | Yes (make `Option` first) | No |
| Rename field | Yes (use `#[serde(alias)]`) | No |
| Change field type | Careful (serde must handle both) | No |

---

## 5. When to Use Which

### Use JSONB when:

- The structure **evolves frequently** (feature flags, user preferences, metadata)
- You need **zero-downtime deploys** with rolling updates
- The data is **read as a whole** (not queried by individual sub-fields in SQL)
- You want **GIN indexing** for containment queries (`@>`, `?`, `?|`)
- Multiple app versions may coexist (microservices, mobile clients)

### Use Composite Types when:

- The structure is **stable and well-defined** (address, coordinates, money+currency)
- You need **database-level type safety** — PostgreSQL validates every field on INSERT
- You want to use the type in **function signatures** or **other composite types**
- You need composite types specifically for **UNNEST-based batch inserts** with a clean single-parameter API
- Schema changes are coordinated and infrequent

### Hybrid: Composite Type with JSONB Fields

AutoModel supports composite types that contain JSONB fields — the best of both worlds:

```sql
CREATE TYPE public.user_with_links_input AS (
    name TEXT,
    email TEXT,
    social_links JSONB  -- flexible sub-structure inside a typed envelope
);
```

This gives you typed batch inserts via `UNNEST(#{items}::user_with_links_input[])` while keeping the JSONB field flexible for evolution. AutoModel generates manual `Encode`/`Decode` impls that handle the `Json` wrapper for the JSONB field automatically.

---

## 6. Best Practice: Use `jsonb` Instead of `jsonb[]`

When storing arrays of structured objects, prefer a **single `jsonb` column** containing a JSON array over a **`jsonb[]`** (PostgreSQL array of jsonb) column.

### The two approaches

```sql
-- RECOMMENDED: Single jsonb column with a JSON array inside
ALTER TABLE public.users ADD COLUMN social_links JSONB DEFAULT '[]'::jsonb;
-- Stores: [{"name": "GitHub", "url": "..."}, {"name": "LinkedIn", "url": "..."}]

-- NOT RECOMMENDED: PostgreSQL array of jsonb values
ALTER TABLE public.users ADD COLUMN tags jsonb[] DEFAULT NULL;
-- Stores: {'{"label": "lang", "value": "rust"}', '{"label": "role", "value": "dev"}'}
```

### Why `jsonb` is better than `jsonb[]`

#### Querying

`jsonb` has rich, well-supported operators for searching inside arrays:

```sql
-- Find users who have a GitHub link (jsonb containment)
SELECT * FROM users WHERE social_links @> '[{"name": "GitHub"}]';

-- Extract all link names (jsonb_array_elements)
SELECT id, elem->>'name' AS link_name
FROM users, jsonb_array_elements(social_links) AS elem;

-- Check if any element has a specific key
SELECT * FROM users WHERE social_links @? '$[*].name ? (@ == "GitHub")';
```

`jsonb[]` requires awkward unnesting of the PostgreSQL array first:

```sql
-- Same query with jsonb[] is more complex
SELECT * FROM users
WHERE EXISTS (
    SELECT 1 FROM unnest(tags) AS tag
    WHERE tag @> '{"label": "lang"}'
);
```

#### GIN Indexing

`jsonb` supports GIN indexes directly and efficiently:

```sql
-- Works: GIN index on jsonb column
CREATE INDEX idx_social_links ON users USING GIN (social_links);

-- Supports @>, ?, ?|, ?& operators out of the box
SELECT * FROM users WHERE social_links @> '[{"name": "GitHub"}]';
-- ^ This query uses the GIN index
```

`jsonb[]` cannot use GIN indexes in the same way. There is no built-in GIN operator class for arrays of jsonb. You would need a functional index or materialized approach:

```sql
-- Does NOT work: no GIN opclass for jsonb[]
CREATE INDEX idx_tags ON users USING GIN (tags);  -- ERROR

-- Workaround: expression index (limited use)
CREATE INDEX idx_tags ON users USING GIN (
    (SELECT jsonb_agg(t) FROM unnest(tags) AS t)
);
-- This is fragile and the planner may not use it
```

#### Batch Insert ergonomics

With `jsonb`, the multiunzip pattern passes each row's value as a standalone `jsonb`:

```sql
-- Clean: each social_links value is one jsonb blob
FROM UNNEST(#{name}::text[], #{email}::text[], #{social_links?}::jsonb[])
```

With `jsonb[]`, batch inserts must juggle two levels of arrays — the outer UNNEST array and the inner jsonb array per row. This leads to the `ARRAY(SELECT jsonb_array_elements(...))` pattern:

```sql
-- Awkward: must reconstruct jsonb[] from flattened jsonb inside UNNEST
INSERT INTO public.users (name, email, tags)
SELECT name, email,
    CASE WHEN tags IS NULL THEN NULL
    ELSE ARRAY(SELECT jsonb_array_elements(tags)) END
FROM UNNEST(#{name}::text[], #{email}::text[], #{tags}::jsonb[])
```

The intermediate encoding flattens each row's `jsonb[]` into a single `jsonb` array for transport, then reconstructs it — adding complexity and potential for subtle bugs with NULLs.

#### Rust type mapping

`jsonb` maps cleanly:

```rust
// Single jsonb column → simple Vec
pub social_links: Option<Vec<UserSocialLink>>
```

`jsonb[]` introduces a nested `Option` layer for element nullability:

```rust
// jsonb[] column → Vec with nullable elements
pub tags: Option<Vec<Option<UserTag>>>
```

The extra `Option` around each element comes from PostgreSQL arrays allowing NULL elements. This infects all code that touches the field — every access needs `.as_ref().map(|tags| tags.iter().filter_map(|t| t.as_ref())...)`.

#### Storage and performance

`jsonb` stores a single TOAST-able value per row. PostgreSQL can compress it efficiently.

`jsonb[]` stores a PostgreSQL array header plus separate TOAST management for each element. For large arrays this is less space-efficient and adds overhead to serialization/deserialization.

### Migration from `jsonb[]` to `jsonb`

If you have an existing `jsonb[]` column and want to migrate to `jsonb`:

```sql
-- 1. Add new jsonb column
ALTER TABLE public.users ADD COLUMN tags_v2 JSONB DEFAULT '[]'::jsonb;

-- 2. Backfill: convert jsonb[] → jsonb array
UPDATE public.users
SET tags_v2 = COALESCE(
    (SELECT jsonb_agg(elem) FROM unnest(tags) AS elem WHERE elem IS NOT NULL),
    '[]'::jsonb
)
WHERE tags IS NOT NULL;

-- 3. After all apps migrated to the new column:
ALTER TABLE public.users DROP COLUMN tags;
ALTER TABLE public.users RENAME COLUMN tags_v2 TO tags;

-- 4. Add GIN index (now possible)
CREATE INDEX idx_users_tags ON users USING GIN (tags);
```

### Summary

| | `jsonb` (JSON array) | `jsonb[]` (PG array of jsonb) |
|---|---|---|
| **GIN index** | Native support | Not directly supported |
| **Query operators** | `@>`, `?`, `?|`, `@?`, `jsonb_path_query` | Requires `unnest()` + subquery |
| **Batch insert** | Clean `::jsonb[]` in UNNEST | Needs `ARRAY(SELECT jsonb_array_elements(...))` |
| **Rust type** | `Vec<T>` | `Vec<Option<T>>` |
| **NULL semantics** | Whole column or empty array | Column NULL, array empty, or individual elements NULL |
| **Storage** | Single TOAST-optimized value | Array header + per-element overhead |
| **jsonb_agg / jsonb_array_elements** | Native | Requires unnest first |

**Recommendation:** Default to `jsonb` with a JSON array inside. Only use `jsonb[]` if you have a specific need for PostgreSQL array semantics (e.g., `array_append`, `array_position`) that outweighs the querying and indexing disadvantages.
