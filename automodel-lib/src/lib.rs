mod codegen;
mod query_definition;
mod query_definition_rt;
mod rust_type;
mod sql_tree;
mod sqlfile_parser;
mod types_extractor;
mod utils;

use query_definition::*;
use query_definition_rt::*;
use sqlfile_parser::*;
use types_extractor::*;

use anyhow::Result;
use std::path::Path;

pub use query_definition::TelemetryLevel;

use crate::codegen::generate_root_module;
use serde::{Deserialize, Serialize};

/// Crate to use for multiunzip operations in batch inserts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiunzipCrate {
    /// Use itertools::multiunzip (supports up to 12 parameters)
    Itertools,
    /// Use many-unzip crate (no parameter limit)
    ManyUnzip,
}

impl Default for MultiunzipCrate {
    fn default() -> Self {
        MultiunzipCrate::Itertools
    }
}

fn default_shard_key_type() -> String {
    "uuid::Uuid".to_string()
}

/// Application-level sharding configuration.
///
/// The mere presence of this block (i.e. a `shard_key`) enables sharding: every
/// generated function is emitted in its sharded form (accepting a
/// `&impl ShardedExecutor` instead of a plain `sqlx::Executor`), and every query
/// is required to have a non-optional parameter matching the shard key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShardingConfig {
    /// Default name of the parameter that determines the shard for a query.
    /// Can be overridden per query via the `shard_key` metadata field.
    pub shard_key: String,
    /// Rust type of the shard key. Defaults to `uuid::Uuid`.
    #[serde(default = "default_shard_key_type")]
    pub key_type: String,
}

/// Default configuration for telemetry and analysis
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefaultsConfig {
    /// Global telemetry defaults
    #[serde(default)]
    pub telemetry: DefaultsTelemetryConfig,
    /// Whether to analyze query performance and warn about sequential scans
    /// Defaults to false
    #[serde(default)]
    pub ensure_indexes: bool,
    /// Global default derive traits applied to all generated structs
    /// These traits are appended by per-query derives configurations
    /// e.g., vec!["Clone".to_string(), "PartialEq".to_string()]
    /// Defaults to empty vec
    #[serde(default)]
    pub derives: DefaultsDerivesConfig,
    /// Crate to use for multiunzip operations in batch inserts
    /// - Itertools: supports up to 12 parameters (default)
    /// - ManyUnzip: no parameter limit, requires many-unzip dependency
    /// Defaults to Itertools
    #[serde(default)]
    pub multiunzip_crate: MultiunzipCrate,
    /// Application-level sharding configuration. When `Some`, sharding is enabled
    /// and every generated function is emitted in its sharded form.
    #[serde(default)]
    pub sharding: Option<ShardingConfig>,
}

/// Default configuration for telemetry and analysis
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefaultsTelemetryConfig {
    /// Global telemetry level
    #[serde(default)]
    pub level: TelemetryLevel,
    /// Whether to include SQL queries as fields in spans by default
    /// Defaults to false
    #[serde(default)]
    pub include_sql: bool,
}

/// Default derive traits configuration for all generated types
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DefaultsDerivesConfig {
    /// Derive traits for return type structs
    /// Defaults to empty vec (Debug is always added)
    #[serde(default)]
    pub return_type: Vec<String>,
    /// Derive traits for parameters structs
    /// Defaults to empty vec (Debug is always added)
    #[serde(default)]
    pub parameters_type: Vec<String>,
    /// Derive traits for conditions structs
    /// Defaults to empty vec (Debug is always added)
    #[serde(default)]
    pub conditions_type: Vec<String>,
    /// Derive traits for error constraint enums
    /// Defaults to empty vec (Debug is always added)
    #[serde(default)]
    pub error_type: Vec<String>,
}

/// Top-level configuration file structure for AutoModel
///
/// Can be loaded from a YAML file and used by both build.rs and the CLI.
///
/// Example `automodel.yml`:
/// ```yaml
/// queries_dir: queries
/// output_dir: src/generated
///
/// telemetry:
///   level: debug
///   include_sql: true
///
/// ensure_indexes: true
///
/// derives:
///   return_type: [Clone]
///   parameters_type: [Clone]
///   conditions_type: [Clone]
///   error_type: [Clone]
///
/// multiunzip_crate: itertools
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoModelConfig {
    /// Directory containing SQL query files
    #[serde(default = "default_queries_dir")]
    pub queries_dir: String,
    /// Output directory for generated Rust code
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    /// Global telemetry defaults
    #[serde(default)]
    pub telemetry: DefaultsTelemetryConfig,
    /// Whether to analyze query performance and warn about sequential scans
    #[serde(default)]
    pub ensure_indexes: bool,
    /// Global default derive traits applied to all generated structs
    #[serde(default)]
    pub derives: DefaultsDerivesConfig,
    /// Crate to use for multiunzip operations in batch inserts
    #[serde(default)]
    pub multiunzip_crate: MultiunzipCrate,
    /// Application-level sharding configuration. Presence enables sharding.
    #[serde(default)]
    pub sharding: Option<ShardingConfig>,
}

fn default_queries_dir() -> String {
    "queries".to_string()
}

fn default_output_dir() -> String {
    "src/generated".to_string()
}

impl AutoModelConfig {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: Self = serde_yaml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;
        Ok(config)
    }

    /// Convert into a `DefaultsConfig` (extracts just the defaults portion)
    pub fn defaults(&self) -> DefaultsConfig {
        DefaultsConfig {
            telemetry: self.telemetry.clone(),
            ensure_indexes: self.ensure_indexes,
            derives: self.derives.clone(),
            multiunzip_crate: self.multiunzip_crate.clone(),
            sharding: self.sharding.clone(),
        }
    }
}

/// Main entry point for the automodel library
pub struct AutoModel {
    queries: Vec<QueryDefinition>,
    defaults: DefaultsConfig,
}

impl AutoModel {
    /// Create a new AutoModel instance by loading queries from SQL files in a directory
    /// with explicit defaults configuration (no YAML file required)
    pub async fn new<P: AsRef<Path>>(queries_dir: P, defaults: DefaultsConfig) -> Result<Self> {
        // Scan SQL files from the queries directory
        let queries = scan_sql_files(queries_dir.as_ref(), defaults.clone()).await?;

        Ok(Self { queries, defaults })
    }

    /// Build script helper for automatically generating code at build time.
    ///
    /// This function should be called from your build.rs script. It will:
    /// - Calculate hash of YAML file and check if generated code is up to date
    /// - If generated code is up to date, skip database connection entirely
    /// - If not up to date and AUTOMODEL_DATABASE_URL is set, regenerate code
    /// - If not up to date and no AUTOMODEL_DATABASE_URL, fail the build
    ///
    /// # Arguments
    ///
    /// * `yaml_file` - Path to the YAML file containing query definitions (relative to build.rs)
    /// * `output_dir` - Path to the directory where module files will be written (relative to build.rs, typically "src/generated")
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// // build.rs
    /// use automodel::AutoModel;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     AutoModel::generate(|| {
    ///         if std::env::var("CI").is_err() {
    ///             std::env::var("AUTOMODEL_DATABASE_URL").map_err(|_| {
    ///                 "AUTOMODEL_DATABASE_URL environment variable must be set for code generation"
    ///                     .to_string()
    ///             })
    ///         } else {
    ///             Err("Detecting not up to date AutoModel generated code in CI environment"
    ///                 .to_string())
    ///         }
    ///     }, "queries", "src/generated", Default::default()).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn generate<F>(
        database_url_cb: F,
        queries_dir: &str,
        output_dir: &str,
        defaults: crate::DefaultsConfig,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnOnce() -> Result<String, String>,
    {
        use sha2::{Digest, Sha256};
        use std::fs;

        println!("cargo:rerun-if-changed={}", output_dir);

        let output_path = Path::new(output_dir);
        let mod_file = output_path.join("mod.rs");
        println!("cargo:rerun-if-changed={}", mod_file.display());

        let mut hasher = Sha256::new();
        hasher.update(env!("CARGO_PKG_VERSION").as_bytes());

        let queries_dir = Path::new(queries_dir);
        if queries_dir.exists() && queries_dir.is_dir() {
            println!("cargo:rerun-if-changed={}", queries_dir.display());
            // Collect all SQL files and sort them for deterministic hashing
            let mut sql_files = Vec::new();
            for module_entry in fs::read_dir(queries_dir)? {
                let module_entry = module_entry?;
                let module_path = module_entry.path();
                if module_path.is_dir() {
                    println!("cargo:rerun-if-changed={}", module_path.display());
                    for sql_entry in fs::read_dir(&module_path)? {
                        let sql_entry = sql_entry?;
                        let sql_path = sql_entry.path();
                        if sql_path.extension().and_then(|e| e.to_str()) == Some("sql") {
                            println!("cargo:rerun-if-changed={}", sql_path.display());
                            sql_files.push(sql_path);
                        }
                    }
                    let output_module_path = output_path.join(module_path.file_name().unwrap());
                    println!("cargo:rerun-if-changed={}.rs", output_module_path.display());
                }
            }

            // Sort for deterministic hashing
            sql_files.sort();

            // Hash each SQL file
            for sql_file in sql_files {
                let sql_contents = fs::read(&sql_file)?;
                hasher.update(&sql_contents);
            }
        }

        let result = hasher.finalize();

        // Convert first 8 bytes of SHA-256 to u64 for a stable hash
        let hash_bytes = &result[0..8];
        let mut hash_u64 = 0u64;
        for (i, &byte) in hash_bytes.iter().enumerate() {
            hash_u64 |= (byte as u64) << (i * 8);
        }
        let source_hash = hash_u64;
        // Check if generated code is up to date
        if Self::is_generated_mod_rs_code_up_to_date(source_hash, &mod_file).unwrap_or(false) {
            println!("cargo:info=Skipping code generation as everything is up to date");

            // Output warnings from file even when skipping build
            let warn_file = output_path.join("automodel.warn");
            if warn_file.exists() {
                if let Ok(warn_content) = fs::read_to_string(&warn_file) {
                    for warning in warn_content.lines() {
                        if !warning.is_empty() {
                            println!("cargo:warning={}", warning);
                        }
                    }
                }
            }

            return Ok(());
        }

        let database_url = database_url_cb().map_err(|e| {
            println!("cargo:error={}", e);
            std::io::Error::new(std::io::ErrorKind::NotConnected, e)
        })?;

        let automodel = AutoModel::new(queries_dir, defaults).await?;
        automodel
            .generate_to_directory(&database_url, output_dir, source_hash)
            .await?;

        Ok(())
    }

    /// Get all unique module names from the loaded queries
    fn get_modules(&self) -> Vec<String> {
        let mut modules: Vec<String> = self
            .queries
            .iter()
            .map(|q| q.module.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        modules.sort();
        modules
    }

    /// Check if generated code is up to date by comparing file hash
    fn is_generated_mod_rs_code_up_to_date<Q: AsRef<Path>>(
        source_hash: u64,
        generated_mod_rs_file: Q,
    ) -> Result<bool> {
        use std::fs;

        // If generated file doesn't exist, we need to regenerate
        if !generated_mod_rs_file.as_ref().exists() {
            return Ok(false);
        }

        // Read first line of generated file to check for hash comment
        let generated_content = fs::read_to_string(generated_mod_rs_file)?;
        let first_line = generated_content.lines().next().unwrap_or("");

        if let Some(hash_comment) = first_line.strip_prefix("// AUTOMODEL_HASH: ") {
            if let Ok(generated_source_hash) = hash_comment.trim().parse::<u64>() {
                return Ok(generated_source_hash == source_hash);
            }
        }

        // No valid hash found, need to regenerate
        Ok(false)
    }

    /// Generate code to output directory with provided database URL
    async fn generate_to_directory(
        &self,
        database_url: &str,
        output_dir: &str,
        source_hash: u64,
    ) -> anyhow::Result<()> {
        use std::fs;
        use std::path::Path;
        use std::time::Duration;

        let output_path = Path::new(output_dir);
        let modules = self.get_modules();

        // Create output directory
        fs::create_dir_all(output_path)?;

        Self::cleanup_unused_files(output_path, &modules)?;

        // Parse connection string and configure timeouts
        let mut config: tokio_postgres::Config = database_url.parse()?;
        config.connect_timeout(Duration::from_secs(10));

        // Connect with NoTls - users should add ?sslmode=disable to their connection string
        // For TLS support, the dependency on postgres-native-tls or tokio-postgres-rustls would be needed
        let (client, connection) = config.connect(tokio_postgres::NoTls).await?;

        // Spawn the connection task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Connection error: {}", e);
            }
        });

        // Temporarily disable sequential scans to force index usage in analysis
        // This helps detect queries that would benefit from indexes even with empty/small tables
        client.execute("SET enable_seqscan = false", &[]).await?;

        // Enforce queries with full path, including schemas
        client.execute("SET search_path TO ''", &[]).await?;

        // PHASE 1: Analyze all queries and collect information
        let analyzed_queries = self.analyze_all_queries(&client).await?;

        // Build TypeSystem from captured statements (no re-preparation needed)
        let type_system = Self::build_type_system(&client, &analyzed_queries).await?;
        type_system.codegen(&output_path.join("types")).await?;

        // Collect all warnings
        let mut all_warnings = Vec::new();

        // PHASE 2: Generate code from analyzed queries (no DB access)
        for module in &modules {
            let (module_code, module_warnings) = crate::codegen::generate_code_for_module(
                &analyzed_queries,
                module,
                &self.defaults,
            )?;
            let module_file = output_path.join(format!("{}.rs", module));
            fs::write(&module_file, &module_code)?;

            // Output warnings for this module
            for warning in &module_warnings {
                println!("cargo:warning={}", warning);
            }
            all_warnings.extend(module_warnings);
        }

        // Create the main mod.rs file
        let sharding_enabled = self.defaults.sharding.is_some();
        let mod_file = output_path.join("mod.rs");
        let mut mod_content = generate_root_module(&modules, source_hash, sharding_enabled);
        mod_content.push_str("pub mod types;\n");
        fs::write(&mod_file, &mod_content)?;

        // Emit the self-contained sharding runtime when sharding is enabled.
        if let Some(sharding) = &self.defaults.sharding {
            let sharding_file = output_path.join("sharding.rs");
            let sharding_code = crate::codegen::generate_sharding_runtime(&sharding.key_type);
            fs::write(&sharding_file, &sharding_code)?;
        }

        // Write all warnings to automodel.warn file only if there are warnings
        let warn_file = output_path.join("automodel.warn");
        if !all_warnings.is_empty() {
            let warn_content = all_warnings.join("\n");
            fs::write(&warn_file, &warn_content)?;
        } else {
            // Delete the file if it exists from a previous run with warnings
            let _ = fs::remove_file(&warn_file);
        }

        Ok(())
    }

    /// Build a TypeSystem from the statements captured during query analysis.
    async fn build_type_system(
        client: &tokio_postgres::Client,
        analyzed_queries: &[QueryDefinitionRuntime],
    ) -> Result<rust_type::TypeSystem> {
        let mut type_system = rust_type::TypeSystem::new();

        for query in analyzed_queries {
            for statement in &query.type_info.statements {
                for param_type in statement.params() {
                    let _ = type_system.insert(param_type);
                }
                for column in statement.columns() {
                    let _ = type_system.insert(column.type_());
                }
            }
        }

        type_system
            .resolve_nullability(client)
            .await
            .unwrap_or_else(|e| eprintln!("Warning: failed to resolve field nullability: {}", e));

        // Apply custom type mappings from query-level `types:` configs.
        //
        // Two key formats:
        //   2-segment: schema.domain_name → alias override (e.g. "public.positive_int": "std::num::NonZeroI32")
        //   3-segment: schema.type.field  → composite field mapping (e.g. "public.users.profile": "UserProfile")
        //
        // Collect all keys from all queries, detect conflicts, then apply.

        // Key: (schema.type_name, field_name) → (mapped_type, needs_json_wrapper, source_query)
        let mut merged_fields: std::collections::HashMap<(String, String), (String, bool, String)> =
            std::collections::HashMap::new();
        // Key: schema.domain_name → (mapped_type, source_query)
        let mut merged_aliases: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();

        for query in analyzed_queries {
            let Some(ref mappings) = query.definition.types else {
                continue;
            };
            for (key, value) in mappings {
                let parts: Vec<&str> = key.splitn(3, '.').collect();
                if parts.len() == 2 {
                    // 2-segment: schema.domain_name → alias override
                    if let Some((existing_type, existing_source)) = merged_aliases.get(key) {
                        if existing_type != value {
                            anyhow::bail!(
                                "Conflicting type mappings for domain `{}`:\n  \
                                 - {}: \"{}\"\n  \
                                 - {}: \"{}\"",
                                key,
                                existing_source,
                                existing_type,
                                query.definition.name,
                                value,
                            );
                        }
                    } else {
                        merged_aliases
                            .insert(key.clone(), (value.clone(), query.definition.name.clone()));
                    }
                } else if parts.len() == 3 {
                    // 3-segment: schema.type.field → composite field mapping
                    let composite_key = format!("{}.{}", parts[0], parts[1]);
                    let field_name = parts[2].to_string();

                    // Parse @json/@native suffix
                    let (clean_type, needs_wrapper) = if value.ends_with("@json") {
                        (&value[..value.len() - 5], true)
                    } else if value.ends_with("@native") {
                        (&value[..value.len() - 7], false)
                    } else {
                        (value.as_str(), true)
                    };

                    let map_key = (composite_key.clone(), field_name.clone());
                    if let Some((existing_type, _, existing_source)) = merged_fields.get(&map_key) {
                        if existing_type != clean_type {
                            anyhow::bail!(
                                "Conflicting type mappings for composite field `{}.{}`:\n  \
                                 - {}: \"{}\"\n  \
                                 - {}: \"{}\"",
                                composite_key,
                                field_name,
                                existing_source,
                                existing_type,
                                query.definition.name,
                                clean_type,
                            );
                        }
                    } else {
                        merged_fields.insert(
                            map_key,
                            (
                                clean_type.to_string(),
                                needs_wrapper,
                                query.definition.name.clone(),
                            ),
                        );
                    }
                }
            }
        }

        // Apply alias mappings (2-segment keys)
        for (key, (mapped_type, _)) in &merged_aliases {
            let parts: Vec<&str> = key.splitn(2, '.').collect();
            type_system.apply_alias_mapping(parts[0], parts[1], mapped_type);
        }

        // Apply composite field mappings (3-segment keys)
        for ((composite_key, field_name), (mapped_type, needs_wrapper, _)) in &merged_fields {
            let parts: Vec<&str> = composite_key.splitn(2, '.').collect();
            type_system.apply_field_mapping(
                parts[0],
                parts[1],
                field_name,
                mapped_type,
                *needs_wrapper,
            );
        }

        Ok(type_system)
    }

    /// PHASE 1: Analyze all queries and extract complete information
    /// This phase interacts with the database to collect all needed information.
    async fn analyze_all_queries(
        &self,
        client: &tokio_postgres::Client,
    ) -> Result<Vec<QueryDefinitionRuntime>> {
        use futures::stream::{self, StreamExt};

        // Process queries in parallel batches of 40
        let analyzed_queries: Vec<QueryDefinitionRuntime> = stream::iter(&self.queries)
            .map(|query| async move {
                println!("cargo:info=Analyzing query '{}'", query.name);

                // Extract type information (captures Statement for later TypeSystem building)
                let explain_variants = query.tree.explain_variants();
                let template = query.tree.template();
                let non_null_columns = query.tree.non_null_columns();
                let type_info = extract_query_types(
                    client,
                    &template,
                    &explain_variants,
                    query.types.as_ref(),
                    &non_null_columns,
                )
                .await?;

                // Analyze query with EXPLAIN to detect mutation and optionally get performance data
                // EXPLAIN fails on mutations (INSERT/UPDATE/DELETE), so we use that to detect them
                // This also pre-computes EXPLAIN params during the analysis phase
                let analysis_result = Self::analyze_query_with_explain(client, query).await?;

                let analyzed_query = QueryDefinitionRuntime::new(
                    query.clone(),
                    type_info,
                    analysis_result.is_mutation,
                    analysis_result.constraints,
                    analysis_result.performance_analysis,
                    analysis_result.explain_params,
                );

                Ok::<_, anyhow::Error>(analyzed_query)
            })
            .buffered(40) // Process up to 40 queries in parallel while preserving order
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        Ok(analyzed_queries)
    }

    /// Analyze query with EXPLAIN to detect mutations and optionally collect performance data
    /// - First checks SQL keywords to quickly identify obvious mutations
    /// - For potential read-only queries: runs EXPLAIN to verify and optionally collect performance
    /// - If EXPLAIN fails on what looks like a SELECT: treat as mutation (edge case)
    async fn analyze_query_with_explain(
        client: &tokio_postgres::Client,
        query: &QueryDefinition,
    ) -> Result<QueryAnalysisResult> {
        // Quick keyword-based detection first
        let template = query.tree.template();
        let sql_upper = template.to_uppercase();
        let sql_trimmed = sql_upper.trim();

        let mutation_keywords = [
            "INSERT", "UPDATE", "DELETE", "TRUNCATE", "DROP", "CREATE", "ALTER",
        ];

        let is_obvious_mutation = mutation_keywords.iter().any(|kw| {
            sql_trimmed.starts_with(kw)
                || (sql_trimmed.starts_with("WITH") && sql_upper.contains(&format!(") {}", kw)))
        });

        if is_obvious_mutation {
            // This is clearly a mutation - extract constraints
            // Use first variant (base query) for constraint extraction
            let query_variants = query.tree.query_variants();
            let (converted_sql, _param_names, _label) = &query_variants[0];
            let constraints = match client.prepare(converted_sql).await {
                Ok(statement) => {
                    match extract_constraints_from_statement(client, &statement, &template).await {
                        Ok(constraints) => constraints,
                        Err(_e) => {
                            // Silently skip constraint extraction errors
                            Vec::new()
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "cargo:info=Failed to prepare statement for constraint extraction for query '{}': {}",
                        query.name, e
                    );
                    Vec::new()
                }
            };

            return Ok(QueryAnalysisResult {
                is_mutation: true,
                performance_analysis: None,
                constraints,
                explain_params: Vec::new(),
            });
        }

        // Pre-compute EXPLAIN parameters for all variants
        let mut explain_params = Vec::new();
        let explain_variants = query.tree.explain_variants();
        for (converted_sql, param_names, _variant_label) in &explain_variants {
            if param_names.is_empty() {
                explain_params.push(None);
            } else {
                match client.prepare(converted_sql).await {
                    Ok(statement) => {
                        let param_types = statement.params();
                        match Self::prepare_explain_params_for_variant(converted_sql, param_types)
                            .await
                        {
                            Ok(params) => explain_params.push(Some(params)),
                            Err(_) => explain_params.push(None),
                        }
                    }
                    Err(_) => explain_params.push(None),
                }
            }
        }

        // Looks like a SELECT or read-only query - verify with EXPLAIN
        let explain_result = if query.ensure_indexes {
            // Run full performance analysis (which includes EXPLAIN)
            Self::analyze_query_performance(client, query).await
        } else {
            // Just run a simple EXPLAIN to verify it's read-only
            Self::detect_mutation_via_explain(client, query, &explain_params).await
        };

        match explain_result {
            Ok(perf_analysis) => {
                // EXPLAIN succeeded - confirmed as read-only query
                let performance = if query.ensure_indexes {
                    Some(perf_analysis)
                } else {
                    None
                };
                Ok(QueryAnalysisResult {
                    is_mutation: false,
                    performance_analysis: performance,
                    constraints: Vec::new(),
                    explain_params,
                })
            }
            Err(_) => {
                // EXPLAIN failed on what looked like a SELECT - treat as mutation (edge case)
                // Warning will be collected in performance analysis
                Ok(QueryAnalysisResult {
                    is_mutation: true,
                    performance_analysis: None,
                    constraints: Vec::new(),
                    explain_params,
                })
            }
        }
    }

    /// Detect if query is a mutation by attempting EXPLAIN (lightweight version)
    /// Returns PerformanceAnalysis with minimal data if EXPLAIN succeeds, otherwise returns error
    async fn detect_mutation_via_explain(
        client: &tokio_postgres::Client,
        query: &QueryDefinition,
        explain_params: &[Option<ExplainParams>],
    ) -> Result<PerformanceAnalysis> {
        let explain_variants = query.tree.explain_variants();
        // Use first variant (all ungrouped blocks + first branch of each group)
        let (_converted_sql, param_names, _label) = &explain_variants[0];

        // Try EXPLAIN with pre-computed parameters
        let explain_result = if !param_names.is_empty() {
            if let Some(params) = &explain_params[0] {
                if params.special_params.is_empty() {
                    // No special params, use dummy params for all parameters
                    let (converted_sql, _param_names, _label) = &explain_variants[0];
                    match client.prepare(converted_sql).await {
                        Ok(statement) => {
                            let param_types = statement.params();
                            let (dummy_params, _) =
                                crate::types_extractor::create_dummy_params(param_types).await?;
                            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                                dummy_params.iter().map(|p| p.as_ref()).collect();
                            client.query(params.explain_sql.as_str(), &param_refs).await
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    // Has special params - some are inlined, others need dummy values
                    // Prepare to get param types for non-special params
                    let (converted_sql, _param_names, _label) = &explain_variants[0];
                    match client.prepare(converted_sql).await {
                        Ok(statement) => {
                            let param_types = statement.params();
                            let (all_dummy_params, _) =
                                crate::types_extractor::create_dummy_params(param_types).await?;

                            // Filter to only non-special params
                            let mut non_special_dummy_params = Vec::new();
                            for (i, dummy_param) in all_dummy_params.iter().enumerate() {
                                if !params.special_params.contains(&i) {
                                    non_special_dummy_params.push(dummy_param.as_ref());
                                }
                            }

                            client
                                .query(params.explain_sql.as_str(), &non_special_dummy_params)
                                .await
                        }
                        Err(e) => Err(e),
                    }
                }
            } else {
                // Pre-computation failed - cannot run EXPLAIN
                return Err(anyhow::anyhow!(
                    "Statement preparation failed, cannot run EXPLAIN"
                ));
            }
        } else {
            // No parameters, execute directly
            let (converted_sql, _, _) = &explain_variants[0];
            let explain_sql = format!("EXPLAIN (FORMAT TEXT, ANALYZE false) {}", converted_sql);
            client.query(&explain_sql, &[]).await
        };

        match explain_result {
            Ok(_) => {
                // EXPLAIN succeeded, so it's a read-only query
                Ok(PerformanceAnalysis {
                    query_name: query.name.clone(),
                    has_sequential_scan: false,
                    sequential_scan_tables: Vec::new(),
                    warnings: Vec::new(),
                    query_plan: None,
                })
            }
            Err(e) => {
                // EXPLAIN failed, likely a mutation query
                Err(anyhow::anyhow!("EXPLAIN failed (likely mutation): {}", e))
            }
        }
    }

    /// Clean up generated files for modules that no longer exist in the YAML config
    fn cleanup_unused_files(
        output_dir: &std::path::Path,
        current_modules: &Vec<String>,
    ) -> Result<(), std::io::Error> {
        use std::fs;

        // Read all files in the output directory
        let entries = fs::read_dir(output_dir)?;

        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Skip mod.rs and non-.rs files
            if file_name_str == "mod.rs" || !file_name_str.ends_with(".rs") {
                continue;
            }

            // Extract module name from filename (remove .rs extension)
            let module_name = &file_name_str[..file_name_str.len() - 3];

            // Check if this module still exists in current YAML config
            if !current_modules.iter().any(|m| m == module_name) {
                let file_path = entry.path();
                fs::remove_file(&file_path)?;
            }
        }

        Ok(())
    }

    /// Prepare EXPLAIN parameters for a single query variant (done once during Phase 1)
    /// Returns ExplainParams to be stored and reused
    async fn prepare_explain_params_for_variant(
        converted_sql: &str,
        param_types: &[tokio_postgres::types::Type],
    ) -> Result<ExplainParams> {
        let (_dummy_params, special_params) =
            crate::types_extractor::create_dummy_params(param_types).await?;

        // Build the EXPLAIN query with special param replacements
        let explain_sql = if special_params.is_empty() {
            format!("EXPLAIN (FORMAT TEXT, ANALYZE false) {}", converted_sql)
        } else {
            // Replace special parameters with casted values and renumber remaining params
            let mut modified_sql = converted_sql.to_string();

            // Replace special parameters from highest index to lowest (to avoid renumbering issues)
            for (param_idx, type_name, value) in special_params.iter().rev() {
                let param_placeholder = format!("${}", param_idx + 1);
                // Don't quote numeric values
                let casted_value = if type_name == "numeric" {
                    format!("{}::{}", value, type_name)
                } else {
                    format!("'{}'::{}", value, type_name)
                };
                modified_sql = modified_sql.replace(&param_placeholder, &casted_value);
            }

            // Renumber remaining parameters
            // Build a mapping of non-special parameter indices
            let mut param_mapping = Vec::new();
            for i in 0..param_types.len() {
                if !special_params.iter().any(|(idx, _, _)| *idx == i) {
                    param_mapping.push(i);
                }
            }

            // Renumber from highest to lowest to avoid conflicts
            for (new_num, &original_idx) in param_mapping.iter().enumerate().rev() {
                let old_placeholder = format!("${}", original_idx + 1);
                let new_placeholder = format!("${}", new_num + 1);
                if old_placeholder != new_placeholder {
                    // Use a temporary placeholder to avoid conflicts
                    let temp_placeholder = format!("__PARAM_{}__", new_num + 1);
                    modified_sql = modified_sql.replace(&old_placeholder, &temp_placeholder);
                }
            }

            // Replace temporary placeholders with final ones
            for (new_num, _) in param_mapping.iter().enumerate() {
                let temp_placeholder = format!("__PARAM_{}__", new_num + 1);
                let final_placeholder = format!("${}", new_num + 1);
                modified_sql = modified_sql.replace(&temp_placeholder, &final_placeholder);
            }

            format!("EXPLAIN (FORMAT TEXT, ANALYZE false) {}", modified_sql)
        };

        Ok(ExplainParams {
            explain_sql,
            special_params: special_params
                .into_iter()
                .map(|(param_index, _, _)| param_index)
                .collect(),
        })
    }
    /// Analyze query performance using EXPLAIN (full analysis with query plan)
    async fn analyze_query_performance(
        client: &tokio_postgres::Client,
        query: &QueryDefinition,
    ) -> Result<PerformanceAnalysis> {
        let mut has_sequential_scan = false;
        let mut sequential_scan_tables = Vec::new();
        let mut warnings = Vec::new();
        let mut full_query_plan = String::new();

        // Choose the variant set used for query-plan coverage:
        // - No choice groups: use `sql_variants` (base with all conditional
        //   blocks removed + each additive `#[...]` block in isolation) so every
        //   optional block gets its own plan/warnings.
        // - With choice groups: use `explain_variants` (all additive blocks
        //   combined with exactly one branch per group) which is the only set
        //   that keeps mutually-exclusive branches as valid, separately-prepared
        //   SQL.
        let plan_variants = if query.tree.derive_choice_groups()?.is_empty() {
            query.tree.query_variants()
        } else {
            query.tree.explain_variants()
        };

        for (i, (converted_sql, param_names, variant_label)) in plan_variants.iter().enumerate() {
            let variant_name = format!("{} ({})", query.name, variant_label);

            // Compute EXPLAIN parameters for this specific variant (special-param
            // inlining + positional renumbering). Aligned to this variant's SQL.
            let variant_explain_params = if param_names.is_empty() {
                None
            } else {
                match client.prepare(converted_sql).await {
                    Ok(statement) => {
                        Self::prepare_explain_params_for_variant(converted_sql, statement.params())
                            .await
                            .ok()
                    }
                    Err(_) => None,
                }
            };

            let (variant_has_seq_scan, variant_tables, variant_warnings, variant_plan) =
                Self::analyze_single_query(
                    client,
                    converted_sql,
                    param_names,
                    &variant_name,
                    variant_explain_params.as_ref(),
                )
                .await?;

            if variant_has_seq_scan {
                has_sequential_scan = true;
                sequential_scan_tables.extend(variant_tables);
            }
            warnings.extend(variant_warnings);

            // Append variant plan to full plan
            if i > 0 {
                full_query_plan.push_str("\n\n");
            }
            if plan_variants.len() > 1 {
                full_query_plan.push_str(&format!("=== {} ===\n", variant_name));
            }
            full_query_plan.push_str(&variant_plan);
        }

        Ok(PerformanceAnalysis {
            query_name: query.name.clone(),
            has_sequential_scan,
            sequential_scan_tables,
            warnings,
            query_plan: if full_query_plan.is_empty() {
                None
            } else {
                Some(full_query_plan)
            },
        })
    }

    /// Analyze a single SQL query variant
    /// sql: already converted to positional parameters ($1, $2, etc.)
    /// param_names: list of parameter names in order
    /// explain_params: pre-computed EXPLAIN SQL and special params
    async fn analyze_single_query(
        client: &tokio_postgres::Client,
        sql: &str,
        param_names: &[String],
        query_name: &str,
        explain_params: Option<&ExplainParams>,
    ) -> Result<(bool, Vec<String>, Vec<String>, String)> {
        let mut has_sequential_scan = false;
        let mut sequential_scan_tables = Vec::new();
        let mut warnings = Vec::new();
        let mut query_plan_lines = Vec::new();

        // Execute EXPLAIN query with appropriate parameters
        let query_result = if !param_names.is_empty() {
            if let Some(params) = explain_params {
                if params.special_params.is_empty() {
                    // No special params, use dummy params
                    match client.prepare(sql).await {
                        Ok(statement) => {
                            let param_types = statement.params();
                            let (dummy_params, _) =
                                crate::types_extractor::create_dummy_params(param_types).await?;
                            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                                dummy_params.iter().map(|p| p.as_ref()).collect();
                            client.query(params.explain_sql.as_str(), &param_refs).await
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to prepare statement for analysis: {}",
                                e
                            ));
                        }
                    }
                } else {
                    // Has special params - some are inlined, others need dummy values
                    match client.prepare(sql).await {
                        Ok(statement) => {
                            let param_types = statement.params();
                            let (all_dummy_params, _) =
                                crate::types_extractor::create_dummy_params(param_types).await?;

                            // Filter to only non-special params
                            let mut non_special_dummy_params = Vec::new();
                            for (i, dummy_param) in all_dummy_params.iter().enumerate() {
                                if !params.special_params.contains(&i) {
                                    non_special_dummy_params.push(dummy_param.as_ref());
                                }
                            }

                            client
                                .query(params.explain_sql.as_str(), &non_special_dummy_params)
                                .await
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to prepare statement for analysis: {}",
                                e
                            ));
                        }
                    }
                }
            } else {
                // Pre-computation failed, try to prepare on-the-fly
                match client.prepare(sql).await {
                    Ok(statement) => {
                        let param_types = statement.params();
                        let (dummy_params, special_params) =
                            crate::types_extractor::create_dummy_params(param_types).await?;

                        if special_params.is_empty() {
                            // No special params, use dummy params directly
                            let explain_sql =
                                format!("EXPLAIN (FORMAT TEXT, ANALYZE false) {}", sql);
                            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                                dummy_params.iter().map(|p| p.as_ref()).collect();
                            client.query(&explain_sql, &param_refs).await
                        } else {
                            // Has special params, inline them
                            let mut modified_sql = sql.to_string();
                            for (param_idx, type_name, value) in special_params.iter().rev() {
                                let param_placeholder = format!("${}", param_idx + 1);
                                let casted_value = if type_name == "numeric" {
                                    format!("{}::{}", value, type_name)
                                } else {
                                    format!("'{}'::{}", value, type_name)
                                };
                                modified_sql =
                                    modified_sql.replace(&param_placeholder, &casted_value);
                            }
                            let explain_sql =
                                format!("EXPLAIN (FORMAT TEXT, ANALYZE false) {}", modified_sql);
                            client.query(&explain_sql, &[]).await
                        }
                    }
                    Err(_) => {
                        return Ok((
                            false,
                            Vec::new(),
                            vec![format!("Query '{}' had EXPLAIN failed", query_name)],
                            String::new(),
                        ));
                    }
                }
            }
        } else {
            // No parameters, execute directly
            let explain_sql = format!("EXPLAIN (FORMAT TEXT, ANALYZE false) {}", sql);
            client.query(&explain_sql, &[]).await
        };

        let Ok(rows) = query_result else {
            let warning = format!("Query '{}' had EXPLAIN failed", query_name);
            return Ok((false, Vec::new(), vec![warning], String::new()));
        };

        // PostgreSQL returns EXPLAIN as text lines

        // State for partition pruning detection: track Append nodes and child scans
        let mut append_indent: Option<usize> = None;
        let mut partition_scans: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for row in rows {
            let plan_line: String = row.get(0);
            query_plan_lines.push(plan_line.clone());

            // --- Partition pruning detection (must run before seq scan to suppress duplicates) ---
            let indent = plan_line.len() - plan_line.trim_start().len();
            let trimmed = plan_line.trim();
            // Strip the "->  " arrow prefix that PostgreSQL uses for child nodes
            let node_name = trimmed.trim_start_matches("->").trim_start();

            // Detect Append / MergeAppend node (signals multi-partition scan)
            if node_name.starts_with("Append") || node_name.starts_with("MergeAppend") {
                // If we had a previous Append, flush it first
                if append_indent.is_some() {
                    Self::emit_partition_pruning_warnings(
                        query_name,
                        &partition_scans,
                        &mut warnings,
                    );
                }
                append_indent = Some(indent);
                partition_scans.clear();
            } else if let Some(ai) = append_indent {
                if indent <= ai {
                    // We've left the Append block — evaluate collected scans
                    Self::emit_partition_pruning_warnings(
                        query_name,
                        &partition_scans,
                        &mut warnings,
                    );
                    append_indent = None;
                    partition_scans.clear();
                } else {
                    // Child node inside Append — extract base table name from table scan nodes
                    // Only count actual table scan nodes, not index scans within them
                    let is_table_scan = node_name.starts_with("Seq Scan")
                        || node_name.starts_with("Index Scan")
                        || node_name.starts_with("Index Only Scan")
                        || node_name.starts_with("Bitmap Heap Scan");
                    if is_table_scan {
                        // Plan lines: "->  Seq Scan on orders_p0 orders_1  (cost=...)"
                        // After " on ": parts[0]=partition, parts[1]=alias (parent table)
                        if let Some(on_pos) = plan_line.find(" on ") {
                            let after_on = &plan_line[on_pos + 4..];
                            let parts: Vec<&str> = after_on.split_whitespace().collect();
                            if let Some(raw_alias) = parts.get(1).or(parts.first()) {
                                // Strip trailing _N suffix from alias to get base table name
                                let base_table = raw_alias
                                    .trim_end_matches(|c: char| c == '_' || c.is_ascii_digit());
                                let base_table = if base_table.is_empty() {
                                    raw_alias.to_string()
                                } else {
                                    base_table.to_string()
                                };
                                *partition_scans.entry(base_table).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }

            // --- Sequential scan detection (skip when inside Append to avoid noisy per-partition warnings) ---
            if plan_line.contains("Seq Scan") && append_indent.is_none() {
                has_sequential_scan = true;

                // Extract table name from the plan line
                // Format is usually "Seq Scan on table_name"
                if let Some(on_pos) = plan_line.find(" on ") {
                    let after_on = &plan_line[on_pos + 4..];
                    let table_name = after_on.split_whitespace().next().unwrap_or("unknown");

                    sequential_scan_tables.push(table_name.to_string());

                    let warning = format!(
                        "Query '{}' performs sequential scan on table '{}'",
                        query_name, table_name
                    );
                    warnings.push(warning);
                }
            }

            // Also check for expensive operations that might indicate missing indexes
            if plan_line.contains("Index Scan") && plan_line.contains("rows=") {
                // This is good - index is being used
            } else if plan_line.contains("Filter:") || plan_line.contains("Sort") {
                // These operations on large tables might benefit from indexes
                // But only report if we haven't already flagged a sequential scan
                if !has_sequential_scan && plan_line.contains("Filter:") {
                    if let Some(on_pos) = plan_line.find(" on ") {
                        let after_on = &plan_line[on_pos + 4..];
                        let table_name = after_on.split_whitespace().next().unwrap_or("unknown");

                        let warning = format!(
                            "Query '{}' uses filtering on table '{}' - verify appropriate indexes exist",
                            query_name, table_name
                        );
                        warnings.push(warning);
                    }
                }
            }
        }

        // Flush any remaining Append block at end of plan
        if append_indent.is_some() {
            Self::emit_partition_pruning_warnings(query_name, &partition_scans, &mut warnings);
        }

        let query_plan = query_plan_lines.join("\n");
        Ok((
            has_sequential_scan,
            sequential_scan_tables,
            warnings,
            query_plan,
        ))
    }

    /// Emit warnings if partition scans indicate missing partition pruning.
    /// Called when we detect an Append node with 2+ child scans on the same base table.
    fn emit_partition_pruning_warnings(
        query_name: &str,
        partition_scans: &std::collections::HashMap<String, usize>,
        warnings: &mut Vec<String>,
    ) {
        for (table, &count) in partition_scans {
            if count >= 2 {
                warnings.push(format!(
                    "Query '{}' scans all {} partitions of table '{}'",
                    query_name, count, table
                ));
            }
        }
    }
}
