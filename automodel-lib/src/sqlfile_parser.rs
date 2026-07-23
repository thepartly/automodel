use crate::query_definition::QueryDefinition;
use crate::query_definition::{ChoiceGroup, ChoiceVariant, NestedChoiceBlock};
use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;

/// Generate SQL query variants for analysis by handling conditional syntax
/// Returns list of (sql, variant_label) tuples
fn generate_query_variants(sql: &str) -> Vec<(String, String)> {
    let mut variants = Vec::new();

    // First variant: remove all conditional blocks #[...]
    let base_query = remove_conditional_blocks(sql);
    if !base_query.trim().is_empty() {
        variants.push((base_query, "base".to_string()));
    }

    // Additional variants: include each conditional block separately
    let conditional_variants = extract_conditional_variants(sql);
    for (i, variant_sql) in conditional_variants.into_iter().enumerate() {
        variants.push((variant_sql, format!("variant {}", i + 1)));
    }

    variants
}

/// Parse a selector directive `#{selector=variant!}` / `#{selector=variant?}`
/// at the very start (after optional whitespace) of a conditional block's
/// content. Returns `(selector, variant, required, directive_char_len)` where
/// `directive_char_len` is the number of characters from the start of
/// `block_content` up to and including the closing `}` (so callers can strip it).
fn parse_selector_directive(block_content: &str) -> Option<(String, String, bool, usize)> {
    let leading_ws: usize = block_content
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();
    let rest: String = block_content.chars().skip(leading_ws).collect();
    if !rest.starts_with("#{") {
        return None;
    }
    // Find the closing '}' of the directive.
    let inner_end = rest.find('}')?;
    let inner = &rest[2..inner_end]; // between "#{" and "}"

    // Must be of the form: IDENT '=' IDENT ('!' | '?')
    let (body, required) = if let Some(stripped) = inner.strip_suffix('!') {
        (stripped, true)
    } else if let Some(stripped) = inner.strip_suffix('?') {
        (stripped, false)
    } else {
        return None;
    };
    let (selector, variant) = body.split_once('=')?;
    let selector = selector.trim();
    let variant = variant.trim();
    if selector.is_empty() || variant.is_empty() {
        return None;
    }
    if !is_valid_rust_identifier(selector) || !is_valid_rust_identifier(variant) {
        return None;
    }

    // Character length consumed: leading whitespace + the "#{...}" directive.
    let directive_char_len = leading_ws + rest[..inner_end + 1].chars().count();
    Some((
        selector.to_string(),
        variant.to_string(),
        required,
        directive_char_len,
    ))
}

/// Split a choice-variant block's content (the text between its outer `#[` `]`,
/// with the selector directive already removed) into its direct parameters and
/// any nested optional `#[...]` blocks (Option B). Direct parameters are those
/// referenced outside every nested block and become mandatory variant fields;
/// each nested block records its exact inner content (so codegen string
/// replacement matches the base SQL) plus its parameters, which become
/// `Option<T>` variant fields.
fn split_choice_variant_content(content: &str) -> (Vec<String>, Vec<NestedChoiceBlock>) {
    let chars: Vec<char> = content.chars().collect();
    let mut nested_blocks: Vec<NestedChoiceBlock> = Vec::new();
    let mut direct_text = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Capture a nested block honoring any further nesting inside it.
            let mut j = i + 2;
            let mut depth = 1;
            let mut inner = String::new();
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '[' => {
                        depth += 1;
                        inner.push('[');
                    }
                    ']' => {
                        depth -= 1;
                        if depth > 0 {
                            inner.push(']');
                        }
                    }
                    c => inner.push(c),
                }
                j += 1;
            }
            let params: Vec<String> =
                crate::types_extractor::parse_parameter_names_from_sql(&inner)
                    .into_iter()
                    .map(|p| strip_param_suffix(&p))
                    .collect();
            nested_blocks.push(NestedChoiceBlock {
                sql_content: inner,
                params,
            });
            i = j;
        } else {
            direct_text.push(chars[i]);
            i += 1;
        }
    }
    let direct_params: Vec<String> =
        crate::types_extractor::parse_parameter_names_from_sql(&direct_text)
            .into_iter()
            .map(|p| strip_param_suffix(&p))
            .collect();
    (direct_params, nested_blocks)
}

/// Scan the raw SQL for conditional blocks whose content begins with a selector
/// directive `#{selector=variant!}` / `#{selector=variant?}`, extract the
/// mutually-exclusive choice groups, and return the SQL with those directives
/// stripped (so downstream variant generation, type extraction and parameter
/// ordering never see the synthetic selector token).
///
/// Constraints (validated here):
/// - a query may declare multiple independent choice groups (one enum each),
///   distinguished by selector name;
/// - all branches of a group must agree on the `!`/`?` optionality marker;
/// - variant names within a group must be unique;
/// - a parameter name may not be shared across two different choice groups.
fn extract_choice_groups(sql: &str) -> Result<(String, Vec<ChoiceGroup>)> {
    use std::collections::HashMap;
    let mut cleaned = String::new();
    // Preserve first-seen selector order so generated enum/argument order is stable.
    let mut selector_order: Vec<String> = Vec::new();
    let mut variants_by_selector: HashMap<String, Vec<ChoiceVariant>> = HashMap::new();
    let mut required_by_selector: HashMap<String, bool> = HashMap::new();
    let mut total_blocks = 0usize;

    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Found start of a conditional block; capture its content honoring nesting.
            let mut j = i + 2;
            let mut bracket_count = 1;
            let mut content = String::new();
            while j < chars.len() && bracket_count > 0 {
                match chars[j] {
                    '[' => {
                        bracket_count += 1;
                        content.push('[');
                    }
                    ']' => {
                        bracket_count -= 1;
                        if bracket_count > 0 {
                            content.push(']');
                        }
                    }
                    c => content.push(c),
                }
                j += 1;
            }

            let this_block_index = total_blocks;
            total_blocks += 1;

            if let Some((selector, variant, required, directive_len)) =
                parse_selector_directive(&content)
            {
                // Enforce consistent optionality marker within each selector.
                match required_by_selector.get(&selector) {
                    Some(existing) if *existing != required => {
                        anyhow::bail!(
                            "Choice-group selector '{}' has conflicting optionality markers \
                             ('?' vs '!'); all branches must use the same marker",
                            selector
                        );
                    }
                    None => {
                        required_by_selector.insert(selector.clone(), required);
                        selector_order.push(selector.clone());
                    }
                    _ => {}
                }
                let group_variants = variants_by_selector.entry(selector.clone()).or_default();
                if group_variants.iter().any(|v| v.variant == variant) {
                    anyhow::bail!(
                        "Choice-group selector '{}' declares duplicate variant '{}'",
                        selector,
                        variant
                    );
                }

                // Strip the directive from the block content, keep the rest.
                let stripped: String = content.chars().skip(directive_len).collect();
                // The exact text that will sit between the outer `#[` `]` in the
                // cleaned SQL; nested `#[...]` blocks (Option B) are split out of
                // it so their parameters become optional per-variant fields while
                // the direct parameters become mandatory fields.
                let emitted = stripped.trim_start().to_string();
                let (params, nested_blocks) = split_choice_variant_content(&emitted);

                group_variants.push(ChoiceVariant {
                    variant,
                    block_index: this_block_index,
                    params,
                    nested_blocks,
                });

                // Emit the cleaned block (directive removed).
                cleaned.push_str("#[");
                cleaned.push_str(&emitted);
                cleaned.push(']');
            } else {
                // Ungrouped conditional block — emit verbatim.
                cleaned.push_str("#[");
                cleaned.push_str(&content);
                cleaned.push(']');
            }

            i = j;
        } else {
            cleaned.push(chars[i]);
            i += 1;
        }
    }

    if selector_order.is_empty() {
        return Ok((sql.to_string(), Vec::new()));
    }

    // A choice group may coexist with additive (ungrouped) conditional blocks,
    // and its branches may carry any combination of parameters — including
    // branches with no parameters at all (e.g. `#[#{sort=unsorted!} LIMIT 100]`).
    // Multiple independent groups may also coexist (each becomes its own enum
    // argument). The code generator numbers and binds branch parameters by
    // source-order membership with per-name deduplication, so a parameter is
    // handled correctly whether it appears in one branch (a per-variant field),
    // some branches (a per-variant field on each), or every branch (a shared
    // top-level argument).

    // A parameter may not be shared across two different choice groups: the
    // generator would number and bind it once while it survives in two separate
    // branch blocks, producing an inconsistent statement. Reject it early.
    let mut param_owner: HashMap<String, String> = HashMap::new();
    for selector in &selector_order {
        for variant in &variants_by_selector[selector] {
            for param in variant.all_params() {
                match param_owner.get(&param) {
                    Some(other) if other != selector => {
                        anyhow::bail!(
                            "Parameter '{}' is used by two different choice groups ('{}' and \
                             '{}'); a parameter may belong to at most one choice group",
                            param,
                            other,
                            selector
                        );
                    }
                    _ => {
                        param_owner.insert(param.clone(), selector.clone());
                    }
                }
            }
        }
    }

    let groups = selector_order
        .into_iter()
        .map(|selector| ChoiceGroup {
            required: required_by_selector[&selector],
            variants: variants_by_selector.remove(&selector).unwrap_or_default(),
            selector,
        })
        .collect();
    Ok((cleaned, groups))
}

/// Remove all SQL comments (`--` line comments and `/* ... */` block comments,
/// including nested block comments) from `sql`.
///
/// Comment-like sequences are preserved when they appear inside single-quoted
/// string literals, double-quoted identifiers, or dollar-quoted strings, so
/// that legitimate SQL content is never corrupted. Everything else that looks
/// like a comment is stripped, which makes the rest of the parsing pipeline
/// (parameter scanning, conditional-block extraction, raw-string generation)
/// immune to whatever a user writes in a comment.
fn strip_sql_comments(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0usize;

    while i < n {
        let c = chars[i];

        // Single-quoted string literal ('' is an escaped quote).
        if c == '\'' {
            out.push(c);
            i += 1;
            while i < n {
                out.push(chars[i]);
                if chars[i] == '\'' {
                    if i + 1 < n && chars[i + 1] == '\'' {
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Double-quoted identifier ("" is an escaped quote).
        if c == '"' {
            out.push(c);
            i += 1;
            while i < n {
                out.push(chars[i]);
                if chars[i] == '"' {
                    if i + 1 < n && chars[i + 1] == '"' {
                        out.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Dollar-quoted string: $tag$ ... $tag$ (tag may be empty).
        if c == '$' {
            if let Some(tag_len) = dollar_quote_tag_len(&chars, i) {
                let tag: String = chars[i..i + tag_len].iter().collect();
                out.push_str(&tag);
                i += tag_len;
                while i < n {
                    if chars[i] == '$' && matches_at(&chars, i, &tag) {
                        out.push_str(&tag);
                        i += tag.chars().count();
                        break;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
        }

        // Line comment: -- ... up to (but not including) the newline.
        if c == '-' && i + 1 < n && chars[i + 1] == '-' {
            i += 2;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comment: /* ... */ with Postgres-style nesting.
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            let mut depth = 1usize;
            i += 2;
            while i < n && depth > 0 {
                if chars[i] == '/' && i + 1 < n && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            // Replace the whole comment with a single space to avoid merging tokens.
            out.push(' ');
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

/// If a dollar-quote opening tag begins at `pos`, return its length in `char`s
/// (including both `$` delimiters). A tag is `$$` or `$ident$` where `ident`
/// matches `[A-Za-z_][A-Za-z0-9_]*`. Returns `None` for e.g. positional
/// placeholders like `$1`.
fn dollar_quote_tag_len(chars: &[char], pos: usize) -> Option<usize> {
    if chars.get(pos) != Some(&'$') {
        return None;
    }
    match chars.get(pos + 1) {
        Some('$') => Some(2),
        Some(&c) if c.is_ascii_alphabetic() || c == '_' => {
            let mut j = pos + 2;
            while let Some(&c2) = chars.get(j) {
                if c2.is_ascii_alphanumeric() || c2 == '_' {
                    j += 1;
                } else {
                    break;
                }
            }
            if chars.get(j) == Some(&'$') {
                Some(j + 1 - pos)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Whether the `char` slice starting at `pos` matches the literal `needle`.
fn matches_at(chars: &[char], pos: usize, needle: &str) -> bool {
    let needle_chars: Vec<char> = needle.chars().collect();
    if pos + needle_chars.len() > chars.len() {
        return false;
    }
    chars[pos..pos + needle_chars.len()] == needle_chars[..]
}

/// Strip parameter suffixes (`??[?]`, `?[?]`, `??`, `[?]`, `?`) from a raw name.
fn strip_param_suffix(name: &str) -> String {
    let s = if let Some(stripped) = name.strip_suffix("[?]") {
        stripped
    } else {
        name
    };
    if let Some(stripped) = s.strip_suffix("??") {
        stripped.to_string()
    } else if let Some(stripped) = s.strip_suffix('?') {
        stripped.to_string()
    } else {
        s.to_string()
    }
}

/// Remove all conditional blocks #[...] from SQL (bracket-aware so that nested
/// `#[...]` blocks are removed together with their enclosing block).
fn remove_conditional_blocks(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut result = String::with_capacity(sql.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Skip the whole block honoring nesting.
            let mut depth = 1;
            let mut j = i + 2;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            i = j;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    // Clean up extra whitespace
    result = result.replace("  ", " ").trim().to_string();
    result
}

/// Strip the outermost pair of a nested `#[...]` marker sequence, keeping the
/// inner content. Repeated application inlines every nested block. Used to build
/// a valid "fully included" isolated variant for EXPLAIN/type extraction.
fn inline_nested_markers(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut result = String::with_capacity(sql.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Keep the inner content, drop the `#[` and its matching `]`.
            let mut depth = 1;
            let mut j = i + 2;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '[' => {
                        depth += 1;
                        result.push('[');
                    }
                    ']' => {
                        depth -= 1;
                        if depth > 0 {
                            result.push(']');
                        }
                    }
                    c => result.push(c),
                }
                j += 1;
            }
            i = j;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Extract variants where each conditional block is included
fn extract_conditional_variants(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut variants = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Locate the matching closing bracket for this (possibly nested) block.
            let start_pos = i;
            let mut depth = 1;
            let mut j = i + 2;
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let end_pos = j; // one past the matching ']'
            let conditional_content: String = chars[start_pos + 2..end_pos - 1].iter().collect();

            // Create variant with this conditional block included, then remove
            // all remaining (other) conditional blocks and inline any nested
            // markers left inside the included block so the result is valid SQL.
            let mut variant: String = chars[..start_pos].iter().collect();
            variant.push_str(&conditional_content);
            let tail: String = chars[end_pos..].iter().collect();
            variant.push_str(&tail);
            variant = remove_conditional_blocks(&variant);
            variant = inline_nested_markers(&variant);

            if !variant.trim().is_empty() {
                variants.push(variant);
            }

            i = end_pos;
        } else {
            i += 1;
        }
    }

    variants
}

/// Validates that a module name is a valid Rust identifier
fn validate_module_name(module_name: &str) -> Result<(), String> {
    if module_name.is_empty() {
        return Err("Module name cannot be empty".to_string());
    }

    // Reuse existing validation logic
    if !is_valid_rust_identifier(module_name) {
        // Check specific error cases to provide better error messages
        let first_char = module_name.chars().next().unwrap();
        if !first_char.is_ascii_alphabetic() && first_char != '_' {
            return Err(format!(
                "Module name '{}' must start with a letter or underscore",
                module_name
            ));
        }

        // Check for invalid characters
        for ch in module_name.chars() {
            if !ch.is_ascii_alphanumeric() && ch != '_' {
                return Err(format!(
                    "Module name '{}' contains invalid character '{}'. Only letters, numbers, and underscores are allowed",
                    module_name, ch
                ));
            }
        }

        // If we get here, it must be a reserved keyword
        if is_rust_keyword(module_name) {
            return Err(format!(
                "Module name '{}' is a reserved Rust keyword and cannot be used",
                module_name
            ));
        }

        // Fallback error (should not happen with current logic)
        return Err(format!(
            "Module name '{}' is not a valid Rust identifier",
            module_name
        ));
    }

    Ok(())
}

/// Check if a string is a valid Rust identifier
fn is_valid_rust_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();
    let first = chars.next().unwrap();

    // First character must be a letter or underscore
    if !first.is_alphabetic() && first != '_' {
        return false;
    }

    // Remaining characters must be alphanumeric or underscore
    for c in chars {
        if !c.is_alphanumeric() && c != '_' {
            return false;
        }
    }

    // Check if it's a Rust keyword
    !is_rust_keyword(name)
}

/// Check if a string is a Rust keyword
fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}

/// Auto-quote a `description:` value in the metadata YAML so free text can
/// contain YAML-significant characters (notably `": "`, which would otherwise
/// be read as a nested mapping and abort parsing).
///
/// A plain (unquoted, non-block) value is rewritten into a single double-quoted
/// scalar. Multi-line plain scalars — where the value continues on subsequent
/// lines indented more deeply than the `description:` key — are folded (per YAML
/// line-folding rules) into that one quoted line, so quoting the first line does
/// not orphan the continuation lines. Values that are already quoted (`"`/`'`)
/// or block scalars (`|`/`>`) are left untouched, as is a `description:` with no
/// inline value.
fn quote_plain_description(yaml: &str) -> String {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if let Some((quoted, consumed)) = quote_plain_description_at(&lines, i) {
            out.push(quoted);
            i += consumed;
        } else {
            out.push(lines[i].to_string());
            i += 1;
        }
    }
    out.join("\n")
}

/// Attempt to rewrite the `description:` entry starting at `lines[start]`.
/// Returns `(quoted_line, lines_consumed)` on success, where `lines_consumed`
/// covers the key line plus any folded continuation lines.
fn quote_plain_description_at(lines: &[&str], start: usize) -> Option<(String, usize)> {
    let line = lines[start];
    let indent_len = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_len);
    let value = rest.strip_prefix("description:")?;
    let value = value.trim();
    // Empty (multi-line block form) or already quoted / a block scalar: leave as-is.
    if value.is_empty()
        || value.starts_with('"')
        || value.starts_with('\'')
        || value.starts_with('|')
        || value.starts_with('>')
    {
        return None;
    }

    // Fold continuation lines: a plain multi-line scalar continues on lines
    // indented more deeply than the key, stopping at the first blank line or a
    // line at the same-or-shallower indentation (the next mapping key).
    let mut folded = value.to_string();
    let mut consumed = 1;
    for next in &lines[start + 1..] {
        let next_indent = next.len() - next.trim_start().len();
        let trimmed = next.trim();
        if trimmed.is_empty() || next_indent <= indent_len {
            break;
        }
        folded.push(' ');
        folded.push_str(trimmed);
        consumed += 1;
    }

    let escaped = folded.replace('\\', "\\\\").replace('"', "\\\"");
    Some((format!("{indent}description: \"{escaped}\""), consumed))
}

/// Parse SQL file with embedded YAML metadata in comments
/// Expected format:
/// ```sql
/// -- @automodel
/// --    description: Update user profile
/// --    expect: exactly_one
/// --    types:
/// --      profile: "crate::models::UserProfile"
/// -- @end
///
/// UPDATE users SET profile = #{profile} WHERE id = #{user_id}
/// ```
async fn parse_sql_file(
    path: &Path,
    module: &str,
    name: &str,
    defaults: crate::DefaultsConfig,
) -> Result<QueryDefinition> {
    let content = fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read SQL file: {}", path.display()))?;

    let mut in_metadata = false;
    let mut yaml_lines = Vec::new();
    let mut sql_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "-- @automodel" {
            in_metadata = true;
            continue;
        }

        if trimmed == "-- @end" {
            in_metadata = false;
            continue;
        }

        if in_metadata {
            // Remove leading "-- " or "--" from the line, but preserve indentation after that
            if let Some(yaml_content) = trimmed.strip_prefix("--") {
                // If there's a space after --, remove it, but keep the rest of the spacing
                let yaml_content = if yaml_content.starts_with(' ') {
                    &yaml_content[1..]
                } else {
                    yaml_content
                };
                yaml_lines.push(yaml_content);
            }
        } else {
            // Everything outside the metadata block is SQL. Comments (including
            // full-line ones) are stripped wholesale below via strip_sql_comments.
            sql_lines.push(line);
        }
    }

    // Parse the YAML metadata. Free-text `description` values routinely contain
    // characters that are illegal in an unquoted YAML plain scalar (most
    // commonly `": "`, but also leading indicators like `#`), so auto-quote a
    // single-line description before handing the document to serde.
    let yaml_str = quote_plain_description(&yaml_lines.join("\n"));

    // Create a temporary QueryDefinition with minimal info
    #[derive(Default, serde::Deserialize)]
    struct TelemetryMetadata {
        #[serde(default)]
        pub level: Option<crate::query_definition::TelemetryLevel>,
        #[serde(default)]
        pub include_params: Option<Vec<String>>,
        #[serde(default)]
        pub include_sql: Option<bool>,
    }

    // Create a temporary QueryDefinition with minimal info
    #[derive(serde::Deserialize)]
    struct QueryMetadata {
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        expect: Option<crate::query_definition::ExpectedResult>,
        #[serde(default)]
        types: Option<std::collections::HashMap<String, String>>,
        #[serde(default)]
        telemetry: TelemetryMetadata,
        #[serde(default)]
        ensure_indexes: Option<bool>,
        #[serde(default)]
        multiunzip: Option<bool>,
        #[serde(default)]
        conditions_type: Option<crate::query_definition::ConditionsType>,
        #[serde(default)]
        parameters_type: Option<crate::query_definition::ParametersType>,
        #[serde(default)]
        return_type: Option<String>,
        #[serde(default)]
        error_type: Option<String>,
        #[serde(default)]
        conditions_type_derives: Vec<String>,
        #[serde(default)]
        parameters_type_derives: Vec<String>,
        #[serde(default)]
        return_type_derives: Vec<String>,
        #[serde(default)]
        error_type_derives: Vec<String>,
    }

    let metadata: QueryMetadata = if yaml_str.trim().is_empty() {
        // No metadata provided, use defaults
        serde_yaml::from_str("{}").unwrap()
    } else {
        serde_yaml::from_str(&yaml_str).with_context(|| {
            format!(
                "Failed to parse YAML metadata in SQL file for query '{}'",
                name
            )
        })?
    };

    // Combine SQL lines, strip all SQL comments, then drop the blank lines that
    // the removed comments leave behind, and trim.
    let sql_stripped = strip_sql_comments(&sql_lines.join("\n"));
    let sql_raw = sql_stripped
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    // Keep raw SQL (with {col!} / "col!" syntax) — extract_query_types strips it at build time.
    let sql = sql_raw;

    if sql.is_empty() {
        anyhow::bail!("SQL file contains no SQL query for '{}'", name);
    }

    // Extract mutually-exclusive choice groups (`#{selector=variant!}` directives)
    // and strip those directives so all downstream processing sees clean SQL.
    let (sql, choice_groups) = extract_choice_groups(&sql)
        .with_context(|| format!("Failed to parse choice groups in query '{}'", name))?;

    // Generate SQL variants and convert to positional parameters at parse time.
    // Also strip non-null column cast syntax so runtime SQL is clean.
    let sql_variants_raw = generate_query_variants(&sql);
    let sql_variants: Vec<(String, Vec<String>, String)> = sql_variants_raw
        .into_iter()
        .map(|(variant_sql, variant_label)| {
            let (clean_sql, _) = crate::types_extractor::strip_non_null_column_casts(&variant_sql);
            let (converted_sql, param_names) =
                crate::types_extractor::convert_named_params_to_positional(&clean_sql);
            (converted_sql, param_names, variant_label)
        })
        .collect();

    // Group-aware valid variants used for EXPLAIN validation and type extraction:
    // every ungrouped block always included plus one branch per choice group.
    let explain_variants = crate::types_extractor::generate_explain_variants(&sql, &choice_groups);

    Ok(QueryDefinition {
        name: name.to_string(),
        sql,
        sql_variants,
        explain_variants,
        description: metadata.description,
        module: module.to_string(),
        expect: metadata.expect.unwrap_or_default(),
        types: metadata.types,
        telemetry: crate::query_definition::QueryTelemetryConfig {
            level: metadata.telemetry.level.unwrap_or(defaults.telemetry.level),
            include_params: metadata.telemetry.include_params,
            include_sql: metadata
                .telemetry
                .include_sql
                .unwrap_or(defaults.telemetry.include_sql),
        },
        ensure_indexes: metadata.ensure_indexes.unwrap_or(defaults.ensure_indexes),
        multiunzip: metadata.multiunzip.unwrap_or(false),
        conditions_type: metadata.conditions_type.unwrap_or_default(),
        parameters_type: metadata.parameters_type.unwrap_or_default(),
        return_type: metadata.return_type,
        error_type: metadata.error_type,
        // Merge global defaults with per-query derives (global first, per-query appends)
        conditions_type_derives: {
            let mut derives = defaults.derives.conditions_type.clone();
            derives.extend(metadata.conditions_type_derives);
            derives
        },
        parameters_type_derives: {
            let mut derives = defaults.derives.parameters_type.clone();
            derives.extend(metadata.parameters_type_derives);
            derives
        },
        return_type_derives: {
            let mut derives = defaults.derives.return_type.clone();
            derives.extend(metadata.return_type_derives);
            derives
        },
        error_type_derives: {
            let mut derives = defaults.derives.error_type.clone();
            derives.extend(metadata.error_type_derives);
            derives
        },
        choice_groups,
    })
}

/// Scan for SQL files in a queries directory and load them as QueryDefinitions
/// Directory structure: queries/{module}/{query_name}.sql
pub async fn scan_sql_files(
    queries_dir: &Path,
    defaults: crate::DefaultsConfig,
) -> Result<Vec<QueryDefinition>> {
    let mut queries = Vec::new();

    // Check if queries directory exists
    if !queries_dir.exists() {
        return Ok(queries);
    }

    // Collect all SQL file paths first, then sort them
    let mut all_sql_files = Vec::new();

    // Read all module directories
    let mut module_dirs = fs::read_dir(queries_dir).await.with_context(|| {
        format!(
            "Failed to read queries directory: {}",
            queries_dir.display()
        )
    })?;

    while let Some(module_entry) = module_dirs.next_entry().await? {
        let module_path = module_entry.path();

        if !module_path.is_dir() {
            continue;
        }

        let module_name = module_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid module directory name"))?
            .to_string();

        // Validate module name
        validate_module_name(&module_name).map_err(|e| {
            anyhow::anyhow!("Invalid module directory name '{}': {}", module_name, e)
        })?;

        // Read all SQL files in the module directory
        let mut sql_files_in_module = fs::read_dir(&module_path).await.with_context(|| {
            format!("Failed to read module directory: {}", module_path.display())
        })?;

        while let Some(sql_entry) = sql_files_in_module.next_entry().await? {
            let sql_path = sql_entry.path();

            if sql_path.extension().and_then(|e| e.to_str()) != Some("sql") {
                continue;
            }

            all_sql_files.push((sql_path, module_name.clone()));
        }
    }

    // Sort SQL files by their full path to ensure consistent ordering
    all_sql_files.sort_by(|a, b| a.0.cmp(&b.0));

    // Now process the sorted files
    for (sql_path, module_name) in all_sql_files {
        let file_stem = sql_path
            .file_stem()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid SQL file name"))?;

        // Strip numeric prefix if present (e.g., "01_query_name" -> "query_name")
        let query_name = if let Some(underscore_pos) = file_stem.find('_') {
            let (prefix, name) = file_stem.split_at(underscore_pos);
            // Check if prefix is all digits
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                name.trim_start_matches('_').to_string()
            } else {
                file_stem.to_string()
            }
        } else {
            file_stem.to_string()
        };

        // Validate query name
        if !is_valid_rust_identifier(&query_name) {
            anyhow::bail!(
                "SQL file name '{}' is not a valid Rust function name. Use only alphanumeric characters and underscores, and start with a letter or underscore.",
                query_name
            );
        }

        let query_def =
            parse_sql_file(&sql_path, &module_name, &query_name, defaults.clone()).await?;
        queries.push(query_def);
    }

    Ok(queries)
}

#[cfg(test)]
mod choice_group_tests {
    use super::*;

    // NOTE: Happy-path choice-group behavior (pure groups, optional groups,
    // shared/per-variant/paramless branch parameters, and mixing with additive
    // blocks) is exercised end-to-end against a real database by the example-app
    // integration test `test_choice_groups.rs`, driven by the `.sql` files in
    // `example-app/queries/choice_groups/`. The tests below cover parser-only
    // invariants and the invalid inputs that must be rejected at build time
    // (which therefore cannot live in the example-app query set).

    #[test]
    fn no_directives_yields_no_groups() {
        let sql = "SELECT id FROM t WHERE 1 = 1 #[AND id = #{x?}]";
        let (cleaned, groups) = extract_choice_groups(sql).unwrap();
        assert!(groups.is_empty());
        assert_eq!(cleaned, sql);
    }

    #[test]
    fn conflicting_markers_error() {
        let sql = include_str!("../tests/fixtures/choice_groups/invalid_conflicting_markers.sql");
        let err = extract_choice_groups(sql).unwrap_err().to_string();
        assert!(err.contains("conflicting optionality markers"), "{}", err);
    }

    #[test]
    fn multiple_independent_groups_parse_separately() {
        // Two distinct selectors with no shared parameters form two independent
        // choice groups, preserving first-seen selector order.
        let sql = "SELECT 1\n#[#{s=a!} ORDER BY a]\n#[#{t=b!} ORDER BY b]";
        let (_, groups) = extract_choice_groups(sql).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].selector, "s");
        assert_eq!(groups[1].selector, "t");
        assert_eq!(groups[0].variants[0].variant, "a");
        assert_eq!(groups[1].variants[0].variant, "b");
    }

    #[test]
    fn cross_group_shared_param_errors() {
        let sql =
            include_str!("../tests/fixtures/choice_groups/invalid_cross_group_shared_param.sql");
        let err = extract_choice_groups(sql).unwrap_err().to_string();
        assert!(err.contains("two different choice groups"), "{}", err);
    }

    #[test]
    fn duplicate_variant_errors() {
        let sql = include_str!("../tests/fixtures/choice_groups/invalid_duplicate_variant.sql");
        let err = extract_choice_groups(sql).unwrap_err().to_string();
        assert!(err.contains("duplicate variant"), "{}", err);
    }
}

#[cfg(test)]
mod description_quoting_tests {
    use super::*;

    #[test]
    fn quotes_plain_description_with_colon() {
        let yaml = "description: picks one mode: fast or slow\nexpect: multiple";
        let quoted = quote_plain_description(yaml);
        let value: serde_yaml::Value = serde_yaml::from_str(&quoted).unwrap();
        assert_eq!(value["description"], "picks one mode: fast or slow");
        assert_eq!(value["expect"], "multiple");
    }

    #[test]
    fn escapes_embedded_quotes_and_backslashes() {
        let yaml = r#"description: says "hi" with a \ slash"#;
        let quoted = quote_plain_description(yaml);
        let value: serde_yaml::Value = serde_yaml::from_str(&quoted).unwrap();
        assert_eq!(value["description"], r#"says "hi" with a \ slash"#);
    }

    #[test]
    fn preserves_indentation() {
        let yaml = "  description: a: b";
        let quoted = quote_plain_description(yaml);
        assert_eq!(quoted, "  description: \"a: b\"");
    }

    #[test]
    fn leaves_already_quoted_and_block_scalars_untouched() {
        for value in ["\"already\"", "'already'", "|", ">"] {
            let yaml = format!("description: {value}");
            assert_eq!(quote_plain_description(&yaml), yaml);
        }
    }

    #[test]
    fn ignores_non_description_keys() {
        let yaml = "return_type: SomeRow\nexpect: multiple";
        assert_eq!(quote_plain_description(yaml), yaml);
    }

    #[test]
    fn folds_multiline_plain_description() {
        let yaml = "description: Hard-delete a single part row by its primary key. Returns the id\n  if found. Child rows carry no foreign key, so the app clears them separately.\n  TODO restore ON DELETE CASCADE once the ingestor is retired.\nexpect: possible_one\ntypes:\n  brand_id: \"suid_rs::GapcBrandSuid@native\"";
        let quoted = quote_plain_description(yaml);
        let value: serde_yaml::Value = serde_yaml::from_str(&quoted).unwrap();
        assert_eq!(
            value["description"],
            "Hard-delete a single part row by its primary key. Returns the id \
             if found. Child rows carry no foreign key, so the app clears them separately. \
             TODO restore ON DELETE CASCADE once the ingestor is retired."
        );
        assert_eq!(value["expect"], "possible_one");
        assert_eq!(value["types"]["brand_id"], "suid_rs::GapcBrandSuid@native");
    }

    #[test]
    fn folds_multiline_plain_description_with_colon() {
        // A continuation line containing ": " would break native YAML folding;
        // quoting the whole scalar keeps it a single string.
        let yaml =
            "description: first line\n  second line: with a colon\nexpect: multiple";
        let quoted = quote_plain_description(yaml);
        let value: serde_yaml::Value = serde_yaml::from_str(&quoted).unwrap();
        assert_eq!(value["description"], "first line second line: with a colon");
        assert_eq!(value["expect"], "multiple");
    }
}

#[cfg(test)]
mod comment_stripping_tests {
    use super::*;

    #[test]
    fn strips_full_line_comments() {
        let sql = "SELECT id\n-- a comment\nFROM t";
        assert_eq!(strip_sql_comments(sql), "SELECT id\n\nFROM t");
    }

    #[test]
    fn strips_trailing_line_comment() {
        let sql = "SELECT id -- trailing\nFROM t";
        assert_eq!(strip_sql_comments(sql), "SELECT id \nFROM t");
    }

    #[test]
    fn strips_block_comment() {
        let sql = "SELECT /* inline */ id FROM t";
        assert_eq!(strip_sql_comments(sql), "SELECT   id FROM t");
    }

    #[test]
    fn strips_nested_block_comment() {
        let sql = "SELECT /* a /* nested */ b */ id";
        assert_eq!(strip_sql_comments(sql), "SELECT   id");
    }

    #[test]
    fn preserves_double_dash_inside_string_literal() {
        let sql = "SELECT '-- not a comment' AS x";
        assert_eq!(strip_sql_comments(sql), sql);
    }

    #[test]
    fn preserves_block_marker_inside_string_literal() {
        let sql = "SELECT '/* keep */' AS x";
        assert_eq!(strip_sql_comments(sql), sql);
    }

    #[test]
    fn preserves_comment_chars_inside_quoted_identifier() {
        let sql = "SELECT \"weird--col\" FROM t";
        assert_eq!(strip_sql_comments(sql), sql);
    }

    #[test]
    fn preserves_comment_chars_inside_dollar_quote() {
        let sql = "SELECT $$ a -- b /* c */ $$ AS x";
        assert_eq!(strip_sql_comments(sql), sql);
    }

    #[test]
    fn preserves_comment_chars_inside_tagged_dollar_quote() {
        let sql = "SELECT $tag$ -- not stripped $tag$ AS x";
        assert_eq!(strip_sql_comments(sql), sql);
    }

    #[test]
    fn does_not_treat_positional_placeholder_as_dollar_quote() {
        let sql = "SELECT id FROM t WHERE a = $1 -- c\nAND b = $2";
        assert_eq!(
            strip_sql_comments(sql),
            "SELECT id FROM t WHERE a = $1 \nAND b = $2"
        );
    }

    #[test]
    fn keeps_conditional_and_named_params_but_drops_comment_versions() {
        // A comment containing #{...}, #[...] and a lone double-quote must not
        // leak into the stripped SQL.
        let sql =
            "SELECT id FROM t\n-- danger: #{ghost?} #[block] and a \" quote\n#[AND id = #{x?}]";
        let out = strip_sql_comments(sql);
        assert!(!out.contains("ghost"), "{}", out);
        assert!(out.contains("#[AND id = #{x?}]"), "{}", out);
    }
}
