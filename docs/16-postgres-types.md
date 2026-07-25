# Supported PostgreSQL Types

> Reference page. See [Custom Type Mappings](08-custom-types.md) to override any
> of these.

AutoModel supports a comprehensive set of PostgreSQL types with automatic mapping
to Rust types. All types support `Option<T>` for nullable columns.

## Boolean & numeric types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `BOOL` | `bool` |
| `CHAR` | `i8` |
| `INT2` (SMALLINT) | `i16` |
| `INT4` (INTEGER) | `i32` |
| `INT8` (BIGINT) | `i64` |
| `FLOAT4` (REAL) | `f32` |
| `FLOAT8` (DOUBLE PRECISION) | `f64` |
| `NUMERIC`, `DECIMAL` | `rust_decimal::Decimal` |
| `OID`, `REGPROC`, `XID`, `CID` | `u32` |
| `XID8` | `u64` |
| `TID` | `(u32, u32)` |

## String & text types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `TEXT` | `String` |
| `VARCHAR` | `String` |
| `CHAR(n)`, `BPCHAR` | `String` |
| `NAME` | `String` |
| `XML` | `String` |

## Binary & bit types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `BYTEA` | `Vec<u8>` |
| `BIT`, `BIT(n)` | `bit_vec::BitVec` |
| `VARBIT` | `bit_vec::BitVec` |

## Date & time types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `DATE` | `chrono::NaiveDate` |
| `TIME` | `chrono::NaiveTime` |
| `TIMETZ` | `sqlx::postgres::types::PgTimeTz` |
| `TIMESTAMP` | `chrono::NaiveDateTime` |
| `TIMESTAMPTZ` | `chrono::DateTime<chrono::Utc>` |
| `INTERVAL` | `sqlx::postgres::types::PgInterval` |

## Range types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `INT4RANGE` | `sqlx::postgres::types::PgRange<i32>` |
| `INT8RANGE` | `sqlx::postgres::types::PgRange<i64>` |
| `NUMRANGE` | `sqlx::postgres::types::PgRange<rust_decimal::Decimal>` |
| `TSRANGE` | `sqlx::postgres::types::PgRange<chrono::NaiveDateTime>` |
| `TSTZRANGE` | `sqlx::postgres::types::PgRange<chrono::DateTime<chrono::Utc>>` |
| `DATERANGE` | `sqlx::postgres::types::PgRange<chrono::NaiveDate>` |

## Multirange types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `INT4MULTIRANGE` | `serde_json::Value` |
| `INT8MULTIRANGE` | `serde_json::Value` |
| `NUMMULTIRANGE` | `serde_json::Value` |
| `TSMULTIRANGE` | `serde_json::Value` |
| `TSTZMULTIRANGE` | `serde_json::Value` |
| `DATEMULTIRANGE` | `serde_json::Value` |

## Network & address types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `INET` | `std::net::IpAddr` |
| `CIDR` | `std::net::IpAddr` |
| `MACADDR` | `mac_address::MacAddress` |

## Geometric types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `POINT` | `sqlx::postgres::types::PgPoint` |
| `LINE` | `sqlx::postgres::types::PgLine` |
| `LSEG` | `sqlx::postgres::types::PgLseg` |
| `BOX` | `sqlx::postgres::types::PgBox` |
| `PATH` | `sqlx::postgres::types::PgPath` |
| `POLYGON` | `sqlx::postgres::types::PgPolygon` |
| `CIRCLE` | `sqlx::postgres::types::PgCircle` |

## JSON & special types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `JSON` | `serde_json::Value` |
| `JSONB` | `serde_json::Value` |
| `JSONPATH` | `String` |
| `UUID` | `uuid::Uuid` |

## Array types

All types support PostgreSQL arrays with automatic mapping to `Vec<T>`:

| PostgreSQL Array Type | Rust Type |
|----------------------|-----------|
| `BOOL[]` | `Vec<bool>` |
| `INT2[]`, `INT4[]`, `INT8[]` | `Vec<i16>`, `Vec<i32>`, `Vec<i64>` |
| `FLOAT4[]`, `FLOAT8[]` | `Vec<f32>`, `Vec<f64>` |
| `TEXT[]`, `VARCHAR[]` | `Vec<String>` |
| `BYTEA[]` | `Vec<Vec<u8>>` |
| `UUID[]` | `Vec<uuid::Uuid>` |
| `DATE[]`, `TIMESTAMP[]`, `TIMESTAMPTZ[]` | `Vec<chrono::NaiveDate>`, `Vec<chrono::NaiveDateTime>`, `Vec<chrono::DateTime<chrono::Utc>>` |
| `INT4RANGE[]`, `DATERANGE[]`, etc. | `Vec<sqlx::postgres::types::PgRange<T>>` |
| And many more… | See the tables above |

## Full-text search & system types

| PostgreSQL Type | Rust Type |
|----------------|-----------|
| `TSQUERY` | `String` |
| `REGCONFIG`, `REGDICTIONARY`, `REGNAMESPACE`, `REGROLE`, `REGCOLLATION` | `u32` |
| `PG_LSN` | `u64` |
| `ACLITEM` | `String` |

## Custom enum types

PostgreSQL custom enums are automatically detected and mapped to generated Rust
enums with proper encoding/decoding support. See
[Custom Type Mappings](08-custom-types.md) to override the mapping.

## Domain types

PostgreSQL domain types (`CREATE DOMAIN`) are generated as Rust type aliases and
their `CHECK` constraints surface in error enums. See
[Custom Type Mappings](08-custom-types.md#domain-type-alias-mappings).

---

← Previous: [CLI & Workspace Commands](15-cli-reference.md) · [Guide Index](README.md) · Next: [Generated Code, Modules & CI →](17-generated-code-and-ci.md)
