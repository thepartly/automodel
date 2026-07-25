use anyhow::Result;
use std::collections::{HashMap, HashSet};
use tokio_postgres::types::{Kind as PgKind, Type as PgType};
use tokio_postgres::Statement;

use crate::rust_type::{InputParam, OutputColumn, RustName, StructField};
use crate::utils::to_snake_case;

/// Constraint information extracted from database schema
#[derive(Debug, Clone)]
pub struct ConstraintInfo {
    /// Constraint name
    pub name: String,
    /// Constraint type: unique, primary_key, foreign_key, check, not_null
    #[allow(unused)]
    pub constraint_type: String,
    /// Table name
    pub table_name: String,
}

/// Information about a SQL query's input and output types
#[derive(Debug, Clone)]
pub struct QueryTypeInfo {
    /// Input parameter types
    pub input_types: Vec<InputParam>,
    /// Output column types and names
    pub output_types: Vec<OutputColumn>,
    /// The prepared statements whose parameter and column types are harvested to
    /// build the `TypeSystem` — one per EXPLAIN variant derived from the query's
    /// `SqlTree` (see `SqlTree::explain_variants`).
    ///
    /// Which variants are valid is determined structurally from the tree, not by
    /// trial-and-error on prepare failures: every additive `#[...]` block is
    /// always included (independent optional clauses compose into a single
    /// preparable query), while the branches of each mutually-exclusive choice
    /// group are enumerated one group at a time (the others held at their first
    /// branch). A query with no choice groups therefore yields a single
    /// statement; one with choice groups yields one statement per branch, so
    /// every parameter and output column across all branches is still captured.
    pub statements: Vec<Statement>,
}

/// Extract type information from a prepared SQL statement.
///
/// Iterates the pre-computed `explain_variants` — a set of *valid* queries where
/// every ungrouped (additive) conditional block is always included and exactly
/// one branch is chosen from each choice group (varying a single group at a
/// time). For a query with no choice groups this is a single fully-combined
/// query. Each variant is prepared on its own; parameter types are collected by
/// name across all variants and re-ordered to match the parameters' source order
/// in the original SQL, so downstream codegen — which indexes `input_types`
/// positionally against the original SQL — keeps working unchanged.
pub async fn extract_query_types(
    client: &tokio_postgres::Client,
    sql: &str,
    explain_variants: &[(String, Vec<String>, String)],
    field_type_mappings: Option<&HashMap<String, String>>,
    non_null_columns: &HashSet<String>,
) -> Result<QueryTypeInfo> {
    // `sql` is already cast-free (rendered from the canonical tree). The set of
    // forced-non-null output columns is supplied by the caller (also derived
    // from the tree) rather than recomputed here via a separate strip pass.

    // Canonical source order of every parameter (suffixes preserved). Downstream
    // codegen expects `input_types` to line up with this ordering.
    let ordered_param_names = parse_parameter_names_from_sql(sql);

    let mut param_types: HashMap<String, InputParam> = HashMap::new();
    let mut output_types: Vec<OutputColumn> = Vec::new();
    let mut have_output = false;
    let mut statements: Vec<Statement> = Vec::new();
    let mut last_err: Option<anyhow::Error> = None;

    for (converted, names, _label) in explain_variants {
        let statement = match client.prepare(converted).await {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(anyhow::anyhow!("{}: {}", e, converted));
                continue;
            }
        };

        let inputs = extract_input_types(&statement, names, field_type_mappings)?;
        for (name, input) in names.iter().zip(inputs) {
            param_types.entry(name.clone()).or_insert(input);
        }

        if !have_output {
            let outputs =
                extract_output_types(client, &statement, field_type_mappings, non_null_columns)
                    .await?;
            if !outputs.is_empty() {
                output_types = outputs;
                have_output = true;
            }
        }

        statements.push(statement);
    }

    if statements.is_empty() {
        return Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("No query variant could be prepared"))
            .context("Failed to prepare any statement for type extraction"));
    }

    // Re-assemble input types in the original source order.
    let mut input_types = Vec::with_capacity(ordered_param_names.len());
    for name in &ordered_param_names {
        if let Some(param) = param_types.get(name) {
            input_types.push(param.clone());
        }
    }

    Ok(QueryTypeInfo {
        input_types,
        output_types,
        statements,
    })
}

/// Extract constraint information from tables involved in a prepared statement
/// This analyzes the statement to identify affected tables and retrieves their constraints
pub async fn extract_constraints_from_statement(
    client: &tokio_postgres::Client,
    statement: &Statement,
    sql: &str,
) -> Result<Vec<ConstraintInfo>> {
    let mut constraints = Vec::new();
    let mut table_oids = HashSet::new();

    // Collect table OIDs from input parameters (for INSERT/UPDATE)
    for param in statement.params() {
        // Skip non-table types
        if let Some(oid) = get_table_oid_from_type(param) {
            table_oids.insert(oid);
        }
    }

    // Collect table OIDs from output columns (for SELECT/RETURNING)
    for column in statement.columns() {
        if let Some(oid) = column.table_oid() {
            table_oids.insert(oid);
        }
    }

    // Fallback: parse SQL to extract table names for DDL statements
    if table_oids.is_empty() {
        let extracted_tables = extract_table_names_from_sql(sql);
        for table_name in extracted_tables {
            if let Some(oid) = get_table_oid_by_name(client, &table_name).await? {
                table_oids.insert(oid);
            }
        }
    }

    // Query constraints for each table
    for table_oid in table_oids {
        let table_constraints = query_table_constraints(client, table_oid).await?;
        constraints.extend(table_constraints);
    }

    Ok(constraints)
}

/// Get table OID from a PostgreSQL type (if it's a composite type)
fn get_table_oid_from_type(_pg_type: &PgType) -> Option<u32> {
    // For now, we can't easily determine table OID from type alone
    // This would require more complex analysis
    None
}

/// Extract table names from SQL using simple pattern matching
fn extract_table_names_from_sql(sql: &str) -> Vec<String> {
    let mut tables = Vec::new();
    let sql_upper = sql.to_uppercase();

    // Simple regex-like patterns for common SQL operations
    let patterns = [
        ("INSERT INTO ", " "),
        ("UPDATE ", " SET"),
        ("FROM ", " "),
        ("JOIN ", " ON"),
    ];

    for (start_pattern, end_pattern) in patterns {
        if let Some(start_pos) = sql_upper.find(start_pattern) {
            let remaining = &sql[start_pos + start_pattern.len()..];
            if let Some(end_pos) = remaining.to_uppercase().find(end_pattern) {
                let table_part = remaining[..end_pos].trim();
                // Extract just the table name (handle schema.table)
                let table_name = table_part
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !table_name.is_empty() && !tables.contains(&table_name.to_string()) {
                    tables.push(table_name.to_string());
                }
            }
        }
    }

    tables
}

/// Get table OID by table name
async fn get_table_oid_by_name(
    client: &tokio_postgres::Client,
    table_name: &str,
) -> Result<Option<u32>> {
    let parts: Vec<&str> = table_name.split('.').collect();
    match parts.as_slice() {
        // Case 1: schema-qualified name: look only there
        [schema, name] => {
            let row = client
                .query_opt(
                    r#"
                    SELECT c.oid
                    FROM pg_class AS c
                    JOIN pg_namespace AS n ON n.oid = c.relnamespace
                    WHERE c.relname = $1
                      AND n.nspname = $2
                    "#,
                    &[&*name, &*schema],
                )
                .await?;
            Ok(row.map(|r| r.get(0)))
        }
        _ => Ok(None),
    }
}

/// Query all constraints for a given table OID
async fn query_table_constraints(
    client: &tokio_postgres::Client,
    table_oid: u32,
) -> Result<Vec<ConstraintInfo>> {
    let mut constraints = Vec::new();

    // Query unique and primary key constraints
    let rows = client
        .query(
            r#"
            SELECT 
                c.conname as constraint_name,
                c.contype::text as constraint_type,
                t.relname as table_name,
                array_agg(a.attname ORDER BY u.attposition) as column_names
            FROM pg_constraint c
            JOIN pg_class t ON c.conrelid = t.oid
            JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS u(attnum, attposition) ON true
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = u.attnum
            WHERE c.conrelid = $1 
                AND c.contype IN ('u', 'p', 'f', 'c')
            GROUP BY c.conname, c.contype, t.relname, c.confrelid, c.confkey
            "#,
            &[&table_oid],
        )
        .await?;

    for row in rows {
        let constraint_name: String = row.get(0);
        let constraint_type_str: String = row.get(1);
        let constraint_type_char: char = constraint_type_str.chars().next().unwrap_or('?');
        let table_name: String = row.get(2);

        let constraint_type = match constraint_type_char {
            'u' => "unique",
            'p' => "primary_key",
            'f' => "foreign_key",
            'c' => "check",
            _ => "other",
        }
        .to_string();

        constraints.push(ConstraintInfo {
            name: constraint_name,
            constraint_type,
            table_name,
        });
    }

    // Query foreign key references separately
    let fk_rows = client
        .query(
            r#"
            SELECT 
                c.conname as constraint_name,
                t.relname as table_name,
                array_agg(a.attname ORDER BY u.attposition) as column_names,
                ft.relname as referenced_table,
                array_agg(fa.attname ORDER BY fu.attposition) as referenced_columns
            FROM pg_constraint c
            JOIN pg_class t ON c.conrelid = t.oid
            JOIN pg_class ft ON c.confrelid = ft.oid
            JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS u(attnum, attposition) ON true
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = u.attnum
            JOIN LATERAL unnest(c.confkey) WITH ORDINALITY AS fu(attnum, attposition) ON true
            JOIN pg_attribute fa ON fa.attrelid = ft.oid AND fa.attnum = fu.attnum
            WHERE c.conrelid = $1 AND c.contype = 'f'
            GROUP BY c.conname, t.relname, ft.relname
            "#,
            &[&table_oid],
        )
        .await?;

    for row in fk_rows {
        let constraint_name: String = row.get(0);
        let table_name: String = row.get(1);

        constraints.push(ConstraintInfo {
            name: constraint_name,
            constraint_type: "foreign_key".to_string(),
            table_name,
        });
    }

    // Query NOT NULL constraints
    let nn_rows = client
        .query(
            r#"
            SELECT 
                a.attname as column_name,
                t.relname as table_name
            FROM pg_attribute a
            JOIN pg_class t ON a.attrelid = t.oid
            WHERE a.attrelid = $1 
                AND a.attnotnull = true
                AND a.attnum > 0
                AND NOT a.attisdropped
            "#,
            &[&table_oid],
        )
        .await?;

    for row in nn_rows {
        let column_name: String = row.get(0);
        let table_name: String = row.get(1);

        constraints.push(ConstraintInfo {
            name: format!("{}_{}_not_null", table_name, column_name),
            constraint_type: "not_null".to_string(),
            table_name,
        });
    }

    // Query domain CHECK constraints for columns that use domain types
    let domain_rows = client
        .query(
            r#"
            SELECT DISTINCT
                dc.conname as constraint_name,
                t.relname as table_name
            FROM pg_attribute a
            JOIN pg_class t ON a.attrelid = t.oid
            JOIN pg_type dt ON a.atttypid = dt.oid AND dt.typtype = 'd'
            JOIN pg_constraint dc ON dc.contypid = dt.oid AND dc.contype = 'c'
            WHERE a.attrelid = $1
                AND a.attnum > 0
                AND NOT a.attisdropped
            "#,
            &[&table_oid],
        )
        .await?;

    for row in domain_rows {
        let constraint_name: String = row.get(0);
        let table_name: String = row.get(1);

        constraints.push(ConstraintInfo {
            name: constraint_name,
            constraint_type: "domain_check".to_string(),
            table_name,
        });
    }

    Ok(constraints)
}

/// Transform a type reference from types-module context to query-module context.
/// `PgType::rust_name()` returns `super::module::Type` for custom PG types,
/// but query modules need `super::types::module::Type`.
fn qualify_type_for_query_module(type_name: &str) -> String {
    type_name.replace("super::", "super::types::")
}

/// Transform `Vec<T>` into `Vec<Option<T>>` for array parameters with nullable elements (`[?]` suffix).
/// If the inner type is already `Option<...>`, returns the input unchanged to avoid double-wrapping.
fn wrap_vec_elements_nullable(type_name: &str) -> String {
    if type_name.starts_with("Vec<") && type_name.ends_with('>') {
        let inner = &type_name[4..type_name.len() - 1];
        if inner.starts_with("Option<") {
            type_name.to_string()
        } else {
            format!("Vec<Option<{}>>", inner)
        }
    } else {
        type_name.to_string()
    }
}

/// Extract input parameter types from a prepared statement
fn extract_input_types(
    statement: &Statement,
    param_names: &[String],
    field_type_mappings: Option<&HashMap<String, String>>,
) -> Result<Vec<InputParam>> {
    let params = statement.params();
    let mut input_types = Vec::new();

    for (i, param_type) in params.iter().enumerate() {
        // Check if this parameter has special suffixes
        let param_name = param_names.get(i).map(|s| s.as_str()).unwrap_or("");

        // Parse suffix combination: ??[?], ?[?], ??, [?], ?, or none
        // [?] = nullable elements (Vec<T> → Vec<Option<T>>)
        // ?  = optional (conditional block inclusion) / nullable (non-conditional)
        // ?? = optional + nullable (conditional block: None=skip, Some(None)=NULL, Some(Some(v))=value)
        let has_element_nullable = param_name.ends_with("[?]");
        let base_for_suffix = if has_element_nullable {
            &param_name[..param_name.len() - 3] // Strip [?]
        } else {
            param_name
        };
        let is_optional_and_nullable = base_for_suffix.ends_with("??");
        let is_optional_param = if is_optional_and_nullable {
            true
        } else {
            base_for_suffix.ends_with('?')
        };
        let is_nullable_value = is_optional_and_nullable;

        // Get clean parameter name (without all suffixes)
        let clean_param_name = if is_optional_and_nullable {
            &base_for_suffix[..base_for_suffix.len() - 2] // Remove ??
        } else if is_optional_param {
            &base_for_suffix[..base_for_suffix.len() - 1] // Remove ?
        } else {
            base_for_suffix
        };

        let base_type_name = qualify_type_for_query_module(
            &param_type
                .rust_name()
                .map_err(|e| anyhow::anyhow!("{}", e))?,
        );

        // For [?] suffix: wrap array element type in Option (Vec<T> -> Vec<Option<T>>)
        let type_ref = if has_element_nullable {
            wrap_vec_elements_nullable(&base_type_name)
        } else {
            base_type_name
        };

        let mut is_optional = is_optional_param;
        let mut needs_json_wrapper = false;
        let mut mapped_type_ref: Option<String> = None;

        // Check if there's a custom type mapping for this parameter
        if let Some(mappings) = field_type_mappings {
            // Look for any mapping that ends with the parameter name
            let custom_type = mappings
                .iter()
                .find(|(key, _)| {
                    // Match patterns like "table.field" or just "field"
                    key.ends_with(&format!(".{}", clean_param_name)) || key == &clean_param_name
                })
                .map(|(_, rust_type_name)| rust_type_name.clone());

            if let Some(custom_type) = custom_type {
                // Reject top-level Option<> in type mappings
                let stripped = custom_type.trim();
                let without_suffix = if stripped.ends_with("@json") {
                    &stripped[..stripped.len() - 5]
                } else if stripped.ends_with("@native") {
                    &stripped[..stripped.len() - 7]
                } else {
                    stripped
                };
                if without_suffix.starts_with("Option<") {
                    anyhow::bail!(
                        "Type mapping for '{}' must not use Option<>. \
                         For output columns, nullability is automatically inferred from the database schema. \
                         For input parameters, use the ?, ??, or [?] suffix annotations in the query.",
                        clean_param_name
                    );
                }

                // Check for explicit JSON wrapper control via @json or @native suffix
                // - Type@json: Force JSON wrapper (for custom types without sqlx traits)
                // - Type@native: No JSON wrapper (for types implementing sqlx::Encode/Decode)
                // - Type (no suffix): Default - JSON wrapper enabled
                let (clean_type, needs_wrapper) = if custom_type.ends_with("@json") {
                    (&custom_type[..custom_type.len() - 5], true)
                } else if custom_type.ends_with("@native") {
                    (&custom_type[..custom_type.len() - 7], false)
                } else {
                    (custom_type.as_str(), true)
                };

                mapped_type_ref = Some(if has_element_nullable {
                    wrap_vec_elements_nullable(clean_type)
                } else {
                    clean_type.to_string()
                });
                needs_json_wrapper = needs_wrapper;
                is_optional = is_optional_param;
            }
        }

        input_types.push(InputParam {
            field: StructField {
                pg_name: clean_param_name.to_string(),
                rust_name: to_snake_case(clean_param_name),
                type_ref,
                mapped_type_ref,
                is_nullable: is_nullable_value,
                needs_json_wrapper,
            },
            is_optional,
            is_nullable_element: has_element_nullable,
        });
    }

    Ok(input_types)
}

/// Per-column metadata resolved from pg_attribute and pg_type system catalogs
struct ColumnMetadata {
    is_nullable: bool,
    /// If the column's actual type is a domain, this holds the domain's Rust name
    domain_type_name: Option<String>,
}

/// Get column metadata (nullability and domain type info) by querying PostgreSQL system catalogs
async fn get_column_metadata(
    client: &tokio_postgres::Client,
    columns: &[tokio_postgres::Column],
) -> Result<Vec<ColumnMetadata>> {
    let mut metadata = Vec::new();

    for column in columns {
        let table_oid = column.table_oid();
        let column_id = column.column_id();

        let meta = if let (Some(table_oid), Some(column_id)) = (table_oid, column_id) {
            // Query pg_attribute + pg_type to get nullability and detect domain types
            let rows = client
                .query(
                    "SELECT a.attnotnull, t.typtype, t.typname, n.nspname \
                     FROM pg_attribute a \
                     JOIN pg_type t ON a.atttypid = t.oid \
                     JOIN pg_namespace n ON t.typnamespace = n.oid \
                     WHERE a.attrelid = $1 AND a.attnum = $2",
                    &[&table_oid, &column_id],
                )
                .await?;

            if let Some(row) = rows.first() {
                let attnotnull: bool = row.get(0);
                let typtype: i8 = row.get(1);
                let typname: String = row.get(2);
                let nspname: String = row.get(3);

                let domain_type_name = if typtype == b'd' as i8 {
                    // Column uses a domain type — produce its qualified Rust name
                    Some(qualify_type_for_query_module(&format!(
                        "super::{}::{}",
                        crate::utils::schema_to_module_name(&nspname),
                        crate::utils::to_pascal_case(&typname),
                    )))
                } else {
                    None
                };

                ColumnMetadata {
                    is_nullable: !attnotnull,
                    domain_type_name,
                }
            } else {
                ColumnMetadata {
                    is_nullable: true,
                    domain_type_name: None,
                }
            }
        } else {
            // No table/column info available (computed column, function result, etc.)
            ColumnMetadata {
                is_nullable: true,
                domain_type_name: None,
            }
        };

        metadata.push(meta);
    }

    Ok(metadata)
}

/// Extract output column types from a prepared statement
async fn extract_output_types(
    client: &tokio_postgres::Client,
    statement: &Statement,
    field_type_mappings: Option<&HashMap<String, String>>,
    non_null_columns: &HashSet<String>,
) -> Result<Vec<OutputColumn>> {
    let columns = statement.columns();
    let mut output_types = Vec::new();

    // Get column metadata (nullability + domain type info)
    let column_metadata = get_column_metadata(client, &columns).await?;

    for (i, column) in columns.iter().enumerate() {
        let column_name = column.name();
        let meta = column_metadata.get(i);
        let is_nullable = meta.map(|m| m.is_nullable).unwrap_or(true);

        // Use domain type name if available, otherwise fall back to the base type
        let base_type_name =
            if let Some(domain_name) = meta.and_then(|m| m.domain_type_name.clone()) {
                domain_name
            } else {
                qualify_type_for_query_module(
                    &column
                        .type_()
                        .rust_name()
                        .map_err(|e| anyhow::anyhow!("{}", e))?,
                )
            };

        // Check if there's a custom type mapping for this field
        let (mapped_type_ref, final_nullable, needs_json_wrapper) = if let Some(mappings) =
            field_type_mappings
        {
            let custom_type = mappings
                .iter()
                .find(|(key, _)| key.ends_with(&format!(".{}", column_name)))
                .map(|(_, rust_type)| rust_type.clone());

            if let Some(custom_type) = custom_type {
                // Reject top-level Option<> in type mappings
                let stripped = custom_type.trim();
                let without_suffix = if stripped.ends_with("@json") {
                    &stripped[..stripped.len() - 5]
                } else if stripped.ends_with("@native") {
                    &stripped[..stripped.len() - 7]
                } else {
                    stripped
                };
                if without_suffix.starts_with("Option<") {
                    anyhow::bail!(
                            "Type mapping for '{}' must not use Option<>. \
                             For output columns, nullability is automatically inferred from the database schema. \
                             For input parameters, use the ?, ??, or [?] suffix annotations in the query.",
                            column_name
                        );
                }

                let (clean_type, needs_wrapper) = if custom_type.ends_with("@json") {
                    (&custom_type[..custom_type.len() - 5], true)
                } else if custom_type.ends_with("@native") {
                    (&custom_type[..custom_type.len() - 7], false)
                } else {
                    (custom_type.as_str(), true)
                };

                (Some(clean_type.to_string()), is_nullable, needs_wrapper)
            } else {
                (None, is_nullable, false)
            }
        } else {
            (None, is_nullable, false)
        };

        let force_non_null = non_null_columns.contains(column_name);
        // Apply non-null override: force_non_null wins over schema-inferred nullability
        let effective_nullable = if force_non_null {
            false
        } else {
            final_nullable
        };

        output_types.push(OutputColumn {
            field: StructField {
                pg_name: column_name.to_string(),
                rust_name: to_snake_case(column_name),
                is_nullable: effective_nullable,
                type_ref: base_type_name,
                mapped_type_ref,
                needs_json_wrapper,
            },
        });
    }

    Ok(output_types)
}

/// Parse SQL to extract meaningful parameter names from named parameters
pub fn parse_parameter_names_from_sql(sql: &str) -> Vec<String> {
    // Look for named parameters in the format #{param_name}
    let mut param_names = Vec::new();
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '#' {
            if let Some(&'{') = chars.peek() {
                chars.next(); // consume the '{'
                let mut param_name = String::new();

                // Read until we find the closing brace
                while let Some(inner_ch) = chars.next() {
                    if inner_ch == '}' {
                        if !param_name.is_empty() {
                            param_names.push(param_name);
                        }
                        break;
                    } else {
                        param_name.push(inner_ch);
                    }
                }
            }
        }
    }

    // If no named parameters found, fall back to counting positional parameters
    if param_names.is_empty() {
        let param_count = sql.matches('$').count();
        param_names = (1..=param_count).map(|i| format!("param_{}", i)).collect();
    }

    param_names
}

/// Create dummy parameter values for EXPLAIN queries
/// Returns (dummy_params, special_params) where special_params contains info about enums and numeric types
pub async fn create_dummy_params(
    param_types: &[tokio_postgres::types::Type],
) -> Result<(
    Vec<Box<dyn tokio_postgres::types::ToSql + Sync>>,
    Vec<(usize, String, String)>,
)> {
    use tokio_postgres::types::Type;

    let mut dummy_params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
    let mut special_params: Vec<(usize, String, String)> = Vec::new(); // (param_index, type_name, value)

    for param_type in param_types {
        // Check if this is an array of composite type (e.g., widgets[])
        if let PgKind::Array(element_type) = param_type.kind() {
            if let PgKind::Composite(_) = element_type.kind() {
                special_params.push((
                    dummy_params.len(),
                    format!("{}[]", element_type.name()),
                    "{}".to_string(),
                ));
                dummy_params.push(Box::new("COMPOSITE_ARRAY_PLACEHOLDER".to_string()));
                continue;
            }
        }

        // Check if this is a direct composite type (e.g., widgets)
        if let PgKind::Composite(_) = param_type.kind() {
            special_params.push((
                dummy_params.len(),
                param_type.name().to_string(),
                "NULL".to_string(),
            ));
            dummy_params.push(Box::new("COMPOSITE_PLACEHOLDER".to_string()));
            continue;
        }

        // Check if this is an enum type
        if let PgKind::Enum(variants) = param_type.kind() {
            let type_name = format!("{}.{}", param_type.schema(), param_type.name());
            special_params.push((dummy_params.len(), type_name, variants[0].clone()));
            dummy_params.push(Box::new("ENUM_PLACEHOLDER".to_string()));
            continue;
        }

        // Handle numeric type specially - PostgreSQL is strict about numeric conversion
        if param_type.name() == "numeric" {
            special_params.push((dummy_params.len(), "numeric".to_string(), "0".to_string()));
            dummy_params.push(Box::new("NUMERIC_PLACEHOLDER".to_string()));
            continue;
        }

        // Handle range types - these need special casting
        if param_type.name().ends_with("range") {
            let type_name = param_type.name();
            special_params.push((
                dummy_params.len(),
                type_name.to_string(),
                "empty".to_string(),
            ));
            dummy_params.push(Box::new("RANGE_PLACEHOLDER".to_string()));
            continue;
        }

        // Handle geometric types - these need special casting
        let geometric_default = match param_type.name() {
            "point" => Some("(0,0)"),
            "line" => Some("{0,0,0}"),
            "lseg" => Some("[(0,0),(0,0)]"),
            "box" => Some("((0,0),(0,0))"),
            "path" => Some("[(0,0)]"),
            "polygon" => Some("((0,0))"),
            "circle" => Some("<(0,0),0>"),
            _ => None,
        };
        if let Some(default_value) = geometric_default {
            let type_name = param_type.name();
            special_params.push((
                dummy_params.len(),
                type_name.to_string(),
                default_value.to_string(),
            ));
            dummy_params.push(Box::new("GEOMETRIC_PLACEHOLDER".to_string()));
            continue;
        }

        // Handle built-in PostgreSQL types
        let dummy_value: Box<dyn tokio_postgres::types::ToSql + Sync> = match param_type {
            // Boolean & Numeric Types
            &Type::BOOL => Box::new(false),
            &Type::CHAR => Box::new(0i8),
            &Type::INT2 => Box::new(0i16),
            &Type::INT4 => Box::new(0i32),
            &Type::INT8 => Box::new(0i64),
            &Type::FLOAT4 => Box::new(0.0f32),
            &Type::FLOAT8 => Box::new(0.0f64),
            &Type::OID | &Type::REGPROC | &Type::XID | &Type::CID => Box::new(0u32),

            // String & Text Types
            &Type::TEXT
            | &Type::VARCHAR
            | &Type::BPCHAR
            | &Type::NAME
            | &Type::XML
            | &Type::UNKNOWN => Box::new("dummy".to_string()),

            // Binary & Bit Types
            &Type::BYTEA => Box::new(vec![0u8]),

            // JSON Types
            &Type::JSON | &Type::JSONB => Box::new(serde_json::Value::Null),

            // Date & Time Types
            &Type::TIMESTAMPTZ => Box::new(chrono::DateTime::from_timestamp(0, 0).unwrap()),
            &Type::TIMESTAMP => {
                Box::new(chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc())
            }
            &Type::DATE => Box::new(chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
            &Type::TIME => Box::new(chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()),

            // UUID
            &Type::UUID => Box::new(uuid::Uuid::nil()),

            // Array types - use empty arrays
            &Type::BOOL_ARRAY => Box::new(Vec::<bool>::new()),
            &Type::CHAR_ARRAY => Box::new(Vec::<i8>::new()),
            &Type::INT2_ARRAY => Box::new(Vec::<i16>::new()),
            &Type::INT4_ARRAY => Box::new(Vec::<i32>::new()),
            &Type::INT8_ARRAY => Box::new(Vec::<i64>::new()),
            &Type::FLOAT4_ARRAY => Box::new(Vec::<f32>::new()),
            &Type::FLOAT8_ARRAY => Box::new(Vec::<f64>::new()),
            &Type::TEXT_ARRAY
            | &Type::VARCHAR_ARRAY
            | &Type::BPCHAR_ARRAY
            | &Type::NAME_ARRAY
            | &Type::XML_ARRAY => Box::new(Vec::<String>::new()),
            &Type::BYTEA_ARRAY => Box::new(Vec::<Vec<u8>>::new()),
            &Type::JSON_ARRAY | &Type::JSONB_ARRAY => Box::new(Vec::<serde_json::Value>::new()),
            &Type::DATE_ARRAY => Box::new(Vec::<chrono::NaiveDate>::new()),
            &Type::TIME_ARRAY => Box::new(Vec::<chrono::NaiveTime>::new()),
            &Type::TIMESTAMP_ARRAY => Box::new(Vec::<chrono::NaiveDateTime>::new()),
            &Type::TIMESTAMPTZ_ARRAY => Box::new(Vec::<chrono::DateTime<chrono::Utc>>::new()),
            &Type::UUID_ARRAY => Box::new(Vec::<uuid::Uuid>::new()),

            // Fallback for unknown types - use string
            _ => Box::new("dummy".to_string()),
        };
        dummy_params.push(dummy_value);
    }

    Ok((dummy_params, special_params))
}
