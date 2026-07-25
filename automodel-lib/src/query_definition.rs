use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Parameters type configuration - can be either a boolean or a struct name
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum ParametersType {
    /// Auto-generate a new struct with name {QueryName}Params
    Enabled(bool),
    /// Use or generate a struct with the given name
    Named(String),
}

impl ParametersType {
    pub fn is_enabled(&self) -> bool {
        match self {
            ParametersType::Enabled(b) => *b,
            ParametersType::Named(_) => true,
        }
    }

    pub fn get_struct_name(&self) -> Option<&str> {
        match self {
            ParametersType::Enabled(_) => None,
            ParametersType::Named(name) => Some(name.as_str()),
        }
    }
}

impl Default for ParametersType {
    fn default() -> Self {
        ParametersType::Enabled(false)
    }
}

/// Conditions type configuration - can be either a boolean or a struct name
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum ConditionsType {
    /// Auto-generate a new struct with name {QueryName}Params
    Enabled(bool),
    /// Use or generate a struct with the given name
    Named(String),
}

impl ConditionsType {
    pub fn is_enabled(&self) -> bool {
        match self {
            ConditionsType::Enabled(b) => *b,
            ConditionsType::Named(_) => true,
        }
    }

    pub fn get_struct_name(&self) -> Option<&str> {
        match self {
            ConditionsType::Enabled(_) => None,
            ConditionsType::Named(name) => Some(name.as_str()),
        }
    }
}

impl Default for ConditionsType {
    fn default() -> Self {
        ConditionsType::Enabled(false)
    }
}

/// Expected result type for a query
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExpectedResult {
    /// Exactly one row must be returned (uses query_one, fails if 0 or >1 rows)
    ExactlyOne,
    /// Zero or one row expected (uses query_opt, returns Option)
    PossibleOne,
    /// At least one row expected (uses query, fails if 0 rows, returns Vec with first element guaranteed)
    AtLeastOne,
    /// Multiple rows expected (uses query, returns Vec which may be empty)
    Multiple,
}

/// OpenTelemetry instrumentation level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryLevel {
    /// No instrumentation
    None,
    /// Basic span creation with function name
    Info,
    /// Include SQL query in span
    Debug,
    /// Include both SQL query and parameters in span
    Trace,
}

impl Default for TelemetryLevel {
    fn default() -> Self {
        TelemetryLevel::None
    }
}

impl Default for ExpectedResult {
    fn default() -> Self {
        ExpectedResult::ExactlyOne
    }
}

/// Represents a single SQL query definition from the YAML file
#[derive(Debug, Clone)]
pub(crate) struct QueryDefinition {
    /// The name of the query, which will be used as the function name
    pub name: String,
    /// Optional description of what the query does
    pub description: Option<String>,
    /// Module name where this function should be generated
    pub module: String,
    /// Expected result type - controls fetch method and error handling
    /// Defaults to "exactly_one" if not specified
    pub expect: ExpectedResult,
    /// Optional per-query field type mappings
    /// Key: field name (e.g., "profile", "metadata", "status")
    /// Value: Rust type to use (e.g., "crate::models::UserProfile", "MyStruct")
    pub types: Option<HashMap<String, String>>,
    /// Optional telemetry configuration for this query
    pub telemetry: QueryTelemetryConfig,
    /// Whether to analyze this query's performance (overrides global setting)
    /// Defaults to None (use global setting)
    pub ensure_indexes: bool,
    /// Whether to use multiunzip pattern for array parameters
    /// When true, the function accepts a Vec of tuples and unzips them into separate arrays
    /// for binding to UNNEST(...) style queries
    /// Defaults to false
    pub multiunzip: bool,
    /// Whether to use diff-based conditional parameters
    /// When true, generates two struct parameters (old and new) and automatically diffs them
    /// When a string, uses or generates a struct with the given name
    /// Defaults to false
    pub conditions_type: ConditionsType,
    /// Type of struct to use for parameters
    /// When true, all query parameters are passed as a single struct
    /// When a string, uses or generates a struct with the given name
    /// Ignored if conditions_type is enabled
    /// Defaults to false
    pub parameters_type: ParametersType,
    /// Type of struct to use for return values
    /// When None or not specified, uses default {QueryName}Item naming
    /// When Some(name), uses or generates a struct with the given name
    pub return_type: Option<String>,
    /// Type of constraint enum to use for errors
    /// When None or not specified, uses default {QueryName}Constraints naming
    /// When Some(name), uses or generates a constraint enum with the given name
    /// Only applies to mutation queries
    pub error_type: Option<String>,
    /// Additional derive traits to add to the conditions struct (conditions_type)
    /// e.g., ["serde::Serialize", "serde::Deserialize"]
    /// Empty vec means no additional derives
    pub conditions_type_derives: Vec<String>,
    /// Additional derive traits to add to the parameters struct (parameters_type)
    /// e.g., ["serde::Serialize", "serde::Deserialize"]
    /// Empty vec means no additional derives
    pub parameters_type_derives: Vec<String>,
    /// Additional derive traits to add to the return type struct
    /// e.g., ["serde::Serialize", "serde::Deserialize"]
    /// Empty vec means no additional derives
    pub return_type_derives: Vec<String>,
    /// Additional derive traits to add to the error constraint enum
    /// e.g., ["serde::Serialize", "serde::Deserialize"]
    /// Empty vec means no additional derives
    pub error_type_derives: Vec<String>,
    /// Canonical token-tree of the query's SQL (comment-free; casts and selector
    /// directives decoded into `Cast` nodes and block gates). The single
    /// structured source of truth from which the SQL template, parameter names,
    /// EXPLAIN/query variants, non-null columns and choice groups are all
    /// rendered on demand.
    pub tree: crate::sql_tree::SqlTree,
}

/// Per-query telemetry configuration
#[derive(Debug, Default, Clone)]
pub(crate) struct QueryTelemetryConfig {
    /// Override global telemetry level for this query
    pub level: TelemetryLevel,
    /// List of input parameter names to include in the span
    /// If not specified or empty, all parameters will be skipped (skip_all)
    pub include_params: Option<Vec<String>>,
    /// Whether to include the SQL query as a field in the span
    /// Defaults to false
    pub include_sql: bool,
}
