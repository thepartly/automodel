# Custom Type Mappings

> Prerequisite: [Metadata Basics](03-metadata-basics.md).

By default AutoModel maps PostgreSQL types to Rust types automatically (see
[Supported PostgreSQL Types](16-postgres-types.md)). The `types:` metadata key
overrides those mappings for specific parameters, columns, composite fields, and
domain types.

## Mapping by name

Map an input parameter or output column to a custom Rust type by its name:

```sql
-- @automodel
--    types:
--      profile: "crate::models::UserProfile"       # parameter or column named "profile"
--      users.profile: "crate::models::UserProfile" # column from a specific table (with JOINs)
--      posts.metadata: "crate::models::PostMetadata"
--      status: "UserStatus"                         # custom enum types
--      category: "crate::enums::Category"
-- @end

SELECT id, name, profile FROM users WHERE id = #{id}
```

## JSON vs native binding

By default a custom type is serialized via JSON. Control this with a suffix:

```sql
-- @automodel
--    types:
--      profile: "UserProfile@json"        # force JSON wrapper (default)
--      uuid: "MyUuid@native"              # no wrapper - type implements sqlx traits
--      data: "Vec<Option<i32>>@native"    # native binding for complex types
-- @end
```

- **`@native`** — the type implements `sqlx::Encode`/`Decode` (or
  `tokio_postgres::ToSql`/`FromSql`).
- **`@json`** or no suffix — uses JSON serialization (requires
  `serde::Serialize`/`Deserialize`).

## Composite type field mappings

Use 3-segment keys (`schema.type.field`) to map fields inside PostgreSQL
composite types. This changes the generated struct field from `serde_json::Value`
to your custom Rust type, wrapped in `sqlx::types::Json<T>`:

```sql
-- @automodel
--    types:
--      public.user_with_links_input.social_links: "Vec<crate::models::UserSocialLink>"
-- @end

INSERT INTO public.users (name, email, social_links)
SELECT r.name, r.email, r.social_links
FROM UNNEST(#{items}::public.user_with_links_input[]) AS r(name, email, social_links)
RETURNING id, name, email, social_links
```

Generates the composite struct with a typed field:

```rust
#[derive(sqlx::Type)]
#[sqlx(type_name = "user_with_links_input")]
pub struct UserWithLinksInput {
    pub name: Option<String>,
    pub email: Option<String>,
    pub social_links: Option<Vec<UserSocialLink>>,
}
```

Key details:

- **`jsonb` fields** → wrapped as `Json<T>` (e.g., `Option<Json<Vec<UserSocialLink>>>`)
- **`jsonb[]` fields** → per-element wrapping as `Vec<Json<T>>` (e.g.,
  `Vec<Option<Json<UserTag>>>`)
- Works for both standalone composite types (`CREATE TYPE`) and table-backed types
- The `@json`/`@native` suffixes apply here too
- Mappings are **global**: if two queries reference the same composite type field,
  both must specify the same target type (conflicting mappings are a build error)
- Multiple queries can contribute mappings for different fields of the same type

```sql
-- Both queries map the same composite type field — types must agree
-- Query A:
--    types:
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"

-- Query B:
--    types:
--      public.users.social_links: "Vec<crate::models::UserSocialLink>"  # OK: same type
--      public.users.profile: "crate::models::UserProfile"               # OK: different field
```

## Domain type alias mappings

PostgreSQL domain types (`CREATE DOMAIN`) are detected automatically and generated
as Rust type aliases:

```sql
CREATE DOMAIN positive_int AS INTEGER CHECK (VALUE > 0);
CREATE DOMAIN email_address AS VARCHAR(255) CHECK (VALUE ~* '^[^@]+@[^@]+$');
```

Generated (default):

```rust
pub type PositiveInt = i32;
pub type EmailAddress = String;
```

Use 2-segment keys (`schema.domain_name`) in `types:` to override the alias
target:

```sql
-- @automodel
--    types:
--      public.positive_int: "std::num::NonZeroI32"
-- @end
```

Generated (with override):

```rust
pub type PositiveInt = std::num::NonZeroI32;
```

Domain `CHECK` constraints are also included in error type enums for mutation
queries (e.g., `PositiveIntCheck`, `EmailAddressCheck`) — see
[Error Handling & Constraints](13-error-handling.md).

## Type mapping key summary

| Key format | Segments | Purpose | Example |
|-----------|----------|---------|---------|
| `field_name` | 1 | Map parameter/column by name | `profile: "UserProfile"` |
| `schema.domain` | 2 | Override domain type alias | `public.positive_int: "NonZeroI32"` |
| `schema.type.field` | 3 | Map composite type field | `public.users.social_links: "Vec<Link>"` |

> **Note:** A top-level `Option<>` in a type mapping is banned. Use the parameter
> [suffixes](04-named-parameters.md) (`?`, `??`, `[?]`) to express optionality and
> nullability instead.

---

← Previous: [Exclusive Choice Blocks](07-choice-blocks.md) · [Guide Index](README.md) · Next: [Non-Null Column Override →](09-non-null-override.md)
