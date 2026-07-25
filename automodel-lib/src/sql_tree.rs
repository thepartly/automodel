//! Canonical token-tree representation of a query's SQL.
//!
//! A [`SqlTree`] is the single structured source of truth for a query: it
//! captures literal text, named parameters (`#{name}`) and conditional blocks
//! (`#[...]`, possibly nested), and — crucially — each block records *why* it is
//! conditional via its [`Gate`] (an additive optional block, or one branch of a
//! mutually-exclusive choice `selector`). Because the gate lives on the node,
//! every projection the rest of the build needs (the cleaned SQL template, the
//! ordered parameter names, the EXPLAIN variants, the per-block `sql_variants`,
//! and the derived choice groups) is a pure function of the tree with no
//! external metadata threaded in.
//!
//! The module is deliberately a **leaf**: it has no dependencies on other
//! `crate::` modules. The choice-group data types ([`ChoiceGroup`],
//! [`ChoiceVariant`], [`NestedChoiceBlock`]), the selector-directive parser
//! ([`parse_selector_directive`]) and the Rust-identifier validators all live
//! here; higher-level modules (`sqlfile_parser`, `codegen`) reuse them from this
//! module. Cast syntax (`{col!}` / `"col!"`) is captured directly as
//! [`SqlToken::Cast`] nodes, so the tree also owns the set of forced-non-null
//! output columns ([`SqlTree::non_null_columns`]) instead of relying on a
//! separate strip pass.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};

/// A single node in the canonical SQL token tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SqlToken {
    /// Verbatim SQL text (everything that is not a directive).
    Lit(String),
    /// A named parameter `#{name...}`, with its optionality/nullability suffixes
    /// captured in structured form.
    Param(Param),
    /// A conditional block `#[...]` together with the condition under which it is
    /// included.
    Block(Block),
    /// A non-null column cast — `{col!}` (native) or `"col!"` (sqlx-compat) — that
    /// forces output column `col` to be non-nullable. Rendered as the bare column
    /// name in every SQL projection; contributes to [`SqlTree::non_null_columns`].
    Cast(Cast),
}

/// A non-null column cast. Its `name` is the bare column identifier rendered into
/// SQL; the cast marks that column as non-nullable for output typing. This is the
/// structured home for in-SQL output annotations (future in-SQL type mappings can
/// grow additional fields here rather than living in the YAML template).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cast {
    /// The bare column name that is both emitted into SQL and marked non-null.
    pub name: String,
}

/// A named parameter `#{name...}` with its trailing suffix decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Param {
    /// The bare parameter name with the suffix removed.
    pub name: String,
    /// The optionality/nullability suffix.
    pub suffix: Suffix,
}

/// The optionality/nullability suffix on a [`Param`].
///
/// The `?`/`??` optionality marker and the `[?]` element-nullable marker are
/// orthogonal, so exactly six combinations are valid — each maps 1:1 to a suffix
/// spelling and to a distinct generated Rust type wrapping:
///
/// | suffix  | variant                         | generated type                   |
/// |---------|---------------------------------|----------------------------------|
/// | (none)  | `None`                          | `T`                              |
/// | `?`     | `Optional`                      | `Option<T>`                      |
/// | `??`    | `OptionalNullable`              | `Option<Option<T>>`              |
/// | `[?]`   | `ElemsNullable`                 | `Vec<Option<T>>`                 |
/// | `?[?]`  | `OptionalElemsNullable`         | `Option<Vec<Option<T>>>`         |
/// | `??[?]` | `OptionalNullableElemsNullable` | `Option<Option<Vec<Option<T>>>>` |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Suffix {
    /// No suffix: the parameter is always bound.
    None,
    /// `?` — optional (controls conditional-block inclusion).
    Optional,
    /// `??` — optional and value-nullable.
    OptionalNullable,
    /// `[?]` — array with nullable elements.
    ElemsNullable,
    /// `?[?]` — optional array with nullable elements.
    OptionalElemsNullable,
    /// `??[?]` — optional, value-nullable array with nullable elements.
    OptionalNullableElemsNullable,
}

impl Suffix {
    /// The literal suffix text, in canonical order (`?`/`??` then `[?]`).
    fn as_str(self) -> &'static str {
        match self {
            Suffix::None => "",
            Suffix::Optional => "?",
            Suffix::OptionalNullable => "??",
            Suffix::ElemsNullable => "[?]",
            Suffix::OptionalElemsNullable => "?[?]",
            Suffix::OptionalNullableElemsNullable => "??[?]",
        }
    }

    /// Decode the suffix from a raw parameter name, returning it alongside the
    /// bare name. Mirrors `sqlfile_parser::strip_param_suffix` exactly: `[?]` is
    /// peeled first, then a trailing `??` or `?`.
    fn split(raw: &str) -> (&str, Suffix) {
        let (rest, elems_nullable) = match raw.strip_suffix("[?]") {
            Some(stripped) => (stripped, true),
            None => (raw, false),
        };
        let (name, optionality) = if let Some(stripped) = rest.strip_suffix("??") {
            (stripped, 2)
        } else if let Some(stripped) = rest.strip_suffix('?') {
            (stripped, 1)
        } else {
            (rest, 0)
        };
        let suffix = match (optionality, elems_nullable) {
            (0, false) => Suffix::None,
            (1, false) => Suffix::Optional,
            (2, false) => Suffix::OptionalNullable,
            (0, true) => Suffix::ElemsNullable,
            (1, true) => Suffix::OptionalElemsNullable,
            (2, true) => Suffix::OptionalNullableElemsNullable,
            _ => unreachable!(),
        };
        (name, suffix)
    }
}

impl Param {
    /// Decode a raw parameter name (as captured between `#{` and `}`) into its
    /// bare name plus structured suffix.
    fn parse(raw: &str) -> Param {
        let (name, suffix) = Suffix::split(raw);
        Param {
            name: name.to_string(),
            suffix,
        }
    }

    /// Reconstruct the raw parameter name with its suffix. Round-trips
    /// [`Param::parse`] for canonical input.
    fn raw(&self) -> String {
        format!("{}{}", self.name, self.suffix.as_str())
    }
}

/// A conditional `#[...]` block: its inclusion condition plus its recursively
/// tokenized body (a nested `#[...]` appears as a nested [`SqlToken::Block`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub gate: Gate,
    pub inner: Vec<SqlToken>,
}

/// The condition that governs whether a [`Block`] is included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Additive block: part of the base statement, included at runtime when its
    /// gating parameters are `Some`. Always included for EXPLAIN validation.
    Optional {
        /// The parameters that must be `Some` for this block to be emitted
        /// (bare names, in source order — including any inside nested blocks).
        /// Codegen gates the block on these, e.g. `if params[0].is_some()`.
        params: Vec<String>,
    },
    /// One branch of a mutually-exclusive choice: included when the generated
    /// `selector` argument equals `variant`. `required` mirrors the optionality
    /// marker (no marker = a branch must be chosen, `?` = the group is
    /// optional).
    Choice {
        selector: String,
        variant: String,
        required: bool,
    },
}

/// The canonical parsed representation of a query's SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqlTree {
    nodes: Vec<SqlToken>,
}

impl SqlTree {
    /// Parse query SQL (comments removed, but cast syntax and selector directives
    /// still present) into a canonical tree.
    ///
    /// Directive handling mirrors the legacy parser exactly: `#[...]` becomes a
    /// [`Block`] (honoring bracket nesting); a block whose content starts with a
    /// `#{selector=variant}` / `?` directive gets a [`Gate::Choice`] and has the
    /// directive stripped (and the remainder `trim_start`ed) from its body, while
    /// every other block gets [`Gate::Optional`]; `#{name}` becomes a
    /// [`SqlToken::Param`] (empty `#{}` is dropped); a `{col!}` / `"col!"` cast
    /// becomes a [`SqlToken::Cast`]; a lone `#` is literal text.
    pub(crate) fn parse(sql: &str) -> Self {
        SqlTree {
            nodes: parse_nodes(sql),
        }
    }

    /// The tree's nodes, for tests/inspection.
    pub(crate) fn nodes(&self) -> &[SqlToken] {
        &self.nodes
    }

    /// Number of top-level conditional blocks (choice + additive), which equals
    /// the count the legacy pipeline assigns block indices to.
    pub(crate) fn top_block_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|t| matches!(t, SqlToken::Block(_)))
            .count()
    }

    /// Every parameter name in source order, with suffixes retained.
    ///
    /// Reproduces `types_extractor::parse_parameter_names_from_sql` on the cleaned
    /// SQL, including its fallback of counting `$` placeholders when no `#{}`
    /// parameters are present.
    pub(crate) fn param_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_params(&self.nodes, &mut out);
        if out.is_empty() {
            let dollars = count_dollars(&self.nodes);
            return (1..=dollars).map(|i| format!("param_{}", i)).collect();
        }
        out
    }

    /// Reconstruct the cleaned SQL template (with `#[...]` / `#{...}` markers, all
    /// selector directives already stripped). Equals the SQL stored by the legacy
    /// parser after choice-group extraction (modulo cast syntax, which the caller
    /// strips before building the tree).
    pub(crate) fn template(&self) -> String {
        let mut out = String::new();
        render_template(&self.nodes, &mut out);
        out
    }

    /// The set of output columns forced non-null via cast syntax (`{col!}` /
    /// `"col!"`), collected from every [`SqlToken::Cast`] in the tree (including
    /// inside conditional blocks). Replaces the second return value of
    /// `types_extractor::strip_non_null_column_casts`.
    pub(crate) fn non_null_columns(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        collect_casts(&self.nodes, &mut out);
        out
    }

    /// The valid variants used for EXPLAIN validation and type extraction.
    ///
    /// Every additive block is always included; for each choice group exactly one
    /// branch is chosen, varying a single group at a time while holding the others
    /// at their first branch (deduplicated; a single all-additive selection when
    /// there are no choice groups). Reproduces
    /// `types_extractor::generate_explain_variants`.
    pub(crate) fn explain_variants(&self) -> Vec<(String, Vec<String>, String)> {
        let block_count = self.top_block_count();

        // Group choice blocks by selector (first-seen order), collecting the
        // block indices per variant (first-seen variant order); a repeated
        // `selector=variant` spans several blocks.
        let mut selector_order: Vec<String> = Vec::new();
        let mut variant_blocks: HashMap<String, Vec<Vec<usize>>> = HashMap::new();
        let mut variant_names: HashMap<String, Vec<String>> = HashMap::new();
        for (this, block) in self.top_blocks() {
            if let Gate::Choice {
                selector, variant, ..
            } = &block.gate
            {
                if !selector_order.contains(selector) {
                    selector_order.push(selector.clone());
                }
                let names = variant_names.entry(selector.clone()).or_default();
                let blocks = variant_blocks.entry(selector.clone()).or_default();
                if let Some(pos) = names.iter().position(|v| v == variant) {
                    blocks[pos].push(this);
                } else {
                    names.push(variant.clone());
                    blocks.push(vec![this]);
                }
            }
        }

        let grouped: HashSet<usize> = variant_blocks
            .values()
            .flat_map(|vs| vs.iter().flatten().copied())
            .collect();
        let additive: Vec<usize> = (0..block_count).filter(|i| !grouped.contains(i)).collect();
        let default_choice: Vec<Vec<usize>> = selector_order
            .iter()
            .map(|s| variant_blocks[s].first().cloned().unwrap_or_default())
            .collect();

        let mut selections: Vec<Vec<usize>> = Vec::new();
        let mut seen: HashSet<Vec<usize>> = HashSet::new();
        for (gi, selector) in selector_order.iter().enumerate() {
            for blocks in &variant_blocks[selector] {
                let mut chosen = default_choice.clone();
                chosen[gi] = blocks.clone();
                let mut included: Vec<usize> = additive.clone();
                for b in &chosen {
                    included.extend(b.iter().copied());
                }
                included.sort_unstable();
                included.dedup();
                if seen.insert(included.clone()) {
                    selections.push(included);
                }
            }
        }
        if selections.is_empty() {
            let mut included = additive.clone();
            included.sort_unstable();
            selections.push(included);
        }

        selections
            .into_iter()
            .enumerate()
            .map(|(i, included)| {
                let set: HashSet<usize> = included.into_iter().collect();
                let (converted, names) = self.render_positional(&set);
                let label = if i == 0 {
                    "base".to_string()
                } else {
                    format!("variant {}", i)
                };
                (converted, names, label)
            })
            .collect()
    }

    /// The per-block `sql_variants`: a base variant with every conditional block
    /// removed, plus one isolated variant per top-level block (that block's direct
    /// body only — nested blocks dropped — every other block removed), each with
    /// whitespace collapsed and parameters converted to `$N`.
    ///
    /// Reproduces `generate_query_variants` followed by the per-variant
    /// `strip_non_null_column_casts` + `convert_named_params_to_positional` applied
    /// in `parse_sql_file` (casts are already rendered as bare column names by the
    /// tree, so the strip is subsumed).
    pub(crate) fn query_variants(&self) -> Vec<(String, Vec<String>, String)> {
        let mut out = Vec::new();

        let base = collapse_ws(&self.render_named(&HashSet::new(), false));
        if !base.is_empty() {
            let (converted, names) = to_positional(&base);
            out.push((converted, names, "base".to_string()));
        }

        let mut label = 1;
        for i in 0..self.top_block_count() {
            let mut included = HashSet::new();
            included.insert(i);
            let collapsed = collapse_ws(&self.render_named(&included, false));
            if collapsed.is_empty() {
                continue;
            }
            let (converted, names) = to_positional(&collapsed);
            out.push((converted, names, format!("variant {}", label)));
            label += 1;
        }

        out
    }

    /// Derive the mutually-exclusive choice groups declared in the tree, applying
    /// the same validations as the legacy `extract_choice_groups`: consistent
    /// optionality (`?` or no marker) per selector, and no parameter shared
    /// across two groups.
    /// Returns groups in first-seen selector order (empty when there are none).
    pub(crate) fn derive_choice_groups(&self) -> Result<Vec<ChoiceGroup>> {
        let mut selector_order: Vec<String> = Vec::new();
        let mut variants_by: HashMap<String, Vec<ChoiceVariant>> = HashMap::new();
        let mut required_by: HashMap<String, bool> = HashMap::new();

        for (this, block) in self.top_blocks() {
            let Gate::Choice {
                selector,
                variant,
                required,
            } = &block.gate
            else {
                continue;
            };

            match required_by.get(selector) {
                Some(existing) if *existing != *required => {
                    bail!(
                        "Choice-group selector '{}' has conflicting optionality markers \
                         (optional '?' vs required); all branches must use the same marker",
                        selector
                    );
                }
                None => {
                    required_by.insert(selector.clone(), *required);
                    selector_order.push(selector.clone());
                }
                _ => {}
            }

            let (params, nested_blocks) = branch_params(&block.inner);
            let group = variants_by.entry(selector.clone()).or_default();
            if let Some(existing) = group.iter_mut().find(|v| v.variant == *variant) {
                existing.block_indices.push(this);
                existing.params.extend(params);
                existing.nested_blocks.extend(nested_blocks);
            } else {
                group.push(ChoiceVariant {
                    variant: variant.clone(),
                    block_indices: vec![this],
                    params,
                    nested_blocks,
                });
            }
        }

        if selector_order.is_empty() {
            return Ok(Vec::new());
        }

        // A parameter may belong to at most one choice group.
        let mut owner: HashMap<String, String> = HashMap::new();
        for selector in &selector_order {
            for variant in &variants_by[selector] {
                for param in variant.all_params() {
                    match owner.get(&param) {
                        Some(other) if other != selector => {
                            bail!(
                                "Parameter '{}' is used by two different choice groups ('{}' and \
                                 '{}'); a parameter may belong to at most one choice group",
                                param,
                                other,
                                selector
                            );
                        }
                        _ => {
                            owner.insert(param.clone(), selector.clone());
                        }
                    }
                }
            }
        }

        Ok(selector_order
            .into_iter()
            .map(|selector| ChoiceGroup {
                required: required_by[&selector],
                variants: variants_by.remove(&selector).unwrap_or_default(),
                selector,
            })
            .collect())
    }

    /// Iterate `(top_level_block_index, &Block)` in source order.
    fn top_blocks(&self) -> impl Iterator<Item = (usize, &Block)> {
        self.nodes
            .iter()
            .filter_map(|t| match t {
                SqlToken::Block(b) => Some(b),
                _ => None,
            })
            .enumerate()
    }

    /// Render a positional (`$N`) SQL string including only the top-level blocks
    /// in `included`, inlining any nested blocks inside an included block.
    /// Reproduces `types_extractor::build_selected_sql`.
    fn render_positional(&self, included: &HashSet<usize>) -> (String, Vec<String>) {
        let mut sql = String::new();
        let mut names = Vec::new();
        let mut counter = 1usize;
        let mut idx = 0usize;
        for node in &self.nodes {
            match node {
                SqlToken::Lit(t) => sql.push_str(t),
                SqlToken::Cast(c) => sql.push_str(&c.name),
                SqlToken::Param(p) => emit_positional(&p.raw(), &mut sql, &mut names, &mut counter),
                SqlToken::Block(b) => {
                    let i = idx;
                    idx += 1;
                    if included.contains(&i) {
                        render_positional_body(&b.inner, &mut sql, &mut names, &mut counter);
                    }
                }
            }
        }
        (sql, names)
    }

    /// Render a `#{name}`-preserving SQL string including only the top-level
    /// blocks in `included`. When `inline_nested` is false, nested blocks inside
    /// an included block are dropped (matching the `sql_variants` isolation
    /// semantics); when true they are inlined.
    fn render_named(&self, included: &HashSet<usize>, inline_nested: bool) -> String {
        let mut out = String::new();
        let mut idx = 0usize;
        for node in &self.nodes {
            match node {
                SqlToken::Lit(t) => out.push_str(t),
                SqlToken::Cast(c) => out.push_str(&c.name),
                SqlToken::Param(p) => push_named(&p.raw(), &mut out),
                SqlToken::Block(b) => {
                    let i = idx;
                    idx += 1;
                    if included.contains(&i) {
                        render_named_body(&b.inner, &mut out, inline_nested);
                    }
                }
            }
        }
        out
    }
}

/// Tokenize `sql` into nodes, detecting selector directives on blocks.
fn parse_nodes(sql: &str) -> Vec<SqlToken> {
    let chars: Vec<char> = sql.chars().collect();
    let mut nodes = Vec::new();
    let mut lit = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            let (inner_raw, next) = capture_block(&chars, i);
            if !lit.is_empty() {
                nodes.push(SqlToken::Lit(std::mem::take(&mut lit)));
            }
            nodes.push(SqlToken::Block(build_block(&inner_raw)));
            i = next;
        } else if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '{' {
            let (name, next) = capture_param(&chars, i);
            if !name.is_empty() {
                if !lit.is_empty() {
                    nodes.push(SqlToken::Lit(std::mem::take(&mut lit)));
                }
                nodes.push(SqlToken::Param(Param::parse(&name)));
            }
            i = next;
        } else if chars[i] == '{' {
            match scan_brace_cast(&chars, i) {
                CastScan::Cast { name, next } => {
                    if !lit.is_empty() {
                        nodes.push(SqlToken::Lit(std::mem::take(&mut lit)));
                    }
                    nodes.push(SqlToken::Cast(Cast { name }));
                    i = next;
                }
                CastScan::Literal { text, next } => {
                    lit.push_str(&text);
                    i = next;
                }
            }
        } else if chars[i] == '"' {
            match scan_quote_cast(&chars, i) {
                CastScan::Cast { name, next } => {
                    if !lit.is_empty() {
                        nodes.push(SqlToken::Lit(std::mem::take(&mut lit)));
                    }
                    nodes.push(SqlToken::Cast(Cast { name }));
                    i = next;
                }
                CastScan::Literal { text, next } => {
                    lit.push_str(&text);
                    i = next;
                }
            }
        } else {
            lit.push(chars[i]);
            i += 1;
        }
    }

    if !lit.is_empty() {
        nodes.push(SqlToken::Lit(lit));
    }
    nodes
}

/// Capture a `#[...]` block starting at `pos` (pointing at `#`). Returns the raw
/// inner content (markers stripped, nested markers preserved) and the index just
/// past the matching `]`.
fn capture_block(chars: &[char], pos: usize) -> (String, usize) {
    let mut depth = 1usize;
    let mut j = pos + 2; // skip '#['
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
    (inner, j)
}

/// Capture a `#{...}` parameter starting at `pos` (pointing at `#`). Returns the
/// name and the index just past the closing `}`.
fn capture_param(chars: &[char], pos: usize) -> (String, usize) {
    let mut j = pos + 2; // skip '#{'
    let mut name = String::new();
    while j < chars.len() {
        if chars[j] == '}' {
            j += 1;
            break;
        }
        name.push(chars[j]);
        j += 1;
    }
    (name, j)
}

/// The outcome of scanning a potential cast token.
enum CastScan {
    /// A valid cast for column `name`; resume scanning at `next`.
    Cast { name: String, next: usize },
    /// Not a cast; `text` is the verbatim scanned span to keep as literal text,
    /// resuming at `next`.
    Literal { text: String, next: usize },
}

/// Scan a native `{col!}` cast starting at `pos` (pointing at `{`, which the
/// caller guarantees is not part of a `#{...}` parameter). Mirrors
/// `types_extractor::strip_non_null_column_casts` syntax 1 exactly: an
/// alphanumeric/`_` identifier, then `!`, then `}`.
fn scan_brace_cast(chars: &[char], pos: usize) -> CastScan {
    let start = pos;
    let mut i = pos + 1; // skip '{'
    let mut name = String::new();
    let mut found_bang = false;
    let mut found_close = false;
    while i < chars.len() {
        if chars[i] == '!' {
            found_bang = true;
            i += 1;
        } else if chars[i] == '}' {
            found_close = true;
            i += 1;
            break;
        } else if found_bang {
            break;
        } else if chars[i].is_ascii_alphanumeric() || chars[i] == '_' {
            name.push(chars[i]);
            i += 1;
        } else {
            break;
        }
    }
    if found_bang && found_close && !name.is_empty() {
        CastScan::Cast { name, next: i }
    } else {
        CastScan::Literal {
            text: chars[start..i].iter().collect(),
            next: i,
        }
    }
}

/// Scan an sqlx-compat `"col!"` cast starting at `pos` (pointing at `"`). Mirrors
/// `types_extractor::strip_non_null_column_casts` syntax 2 exactly: a
/// double-quoted identifier whose last character before the closing quote is `!`.
fn scan_quote_cast(chars: &[char], pos: usize) -> CastScan {
    let start = pos;
    let mut i = pos + 1; // skip opening '"'
    let mut name = String::new();
    let mut found_close = false;
    while i < chars.len() {
        if chars[i] == '"' {
            found_close = true;
            i += 1;
            break;
        }
        name.push(chars[i]);
        i += 1;
    }
    if found_close && name.ends_with('!') {
        let clean = &name[..name.len() - 1];
        if !clean.is_empty() {
            return CastScan::Cast {
                name: clean.to_string(),
                next: i,
            };
        }
    }
    CastScan::Literal {
        text: chars[start..i].iter().collect(),
        next: i,
    }
}

/// Build a [`Block`] from raw block content, splitting off a leading selector
/// directive into a [`Gate::Choice`] (with the directive removed and the body
/// `trim_start`ed) or defaulting to [`Gate::Optional`].
fn build_block(inner_raw: &str) -> Block {
    if let Some((selector, variant, required, directive_len)) = parse_selector_directive(inner_raw)
    {
        let stripped: String = inner_raw.chars().skip(directive_len).collect();
        let emitted = stripped.trim_start();
        Block {
            gate: Gate::Choice {
                selector,
                variant,
                required,
            },
            inner: parse_nodes(emitted),
        }
    } else {
        let inner = parse_nodes(inner_raw);
        let mut params = Vec::new();
        collect_bare_params(&inner, &mut params);
        Block {
            gate: Gate::Optional { params },
            inner,
        }
    }
}

/// Direct branch parameters (suffix-stripped, in source order) and nested
/// optional blocks of a choice branch body, mirroring
/// `sqlfile_parser::split_choice_variant_content`.
fn branch_params(inner: &[SqlToken]) -> (Vec<String>, Vec<NestedChoiceBlock>) {
    let mut direct = Vec::new();
    let mut nested = Vec::new();
    for node in inner {
        match node {
            SqlToken::Param(p) => direct.push(p.name.clone()),
            SqlToken::Block(b) => {
                let mut raw = Vec::new();
                collect_bare_params(&b.inner, &mut raw);
                nested.push(NestedChoiceBlock { params: raw });
            }
            SqlToken::Lit(_) | SqlToken::Cast(_) => {}
        }
    }
    (direct, nested)
}

/// Collect every parameter's raw name (with suffixes) under `nodes`, recursively,
/// in source order.
fn collect_params(nodes: &[SqlToken], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            SqlToken::Param(p) => out.push(p.raw()),
            SqlToken::Block(b) => collect_params(&b.inner, out),
            SqlToken::Lit(_) | SqlToken::Cast(_) => {}
        }
    }
}

/// Collect every parameter's bare name (suffixes stripped) under `nodes`,
/// recursively, in source order.
fn collect_bare_params(nodes: &[SqlToken], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            SqlToken::Param(p) => out.push(p.name.clone()),
            SqlToken::Block(b) => collect_bare_params(&b.inner, out),
            SqlToken::Lit(_) | SqlToken::Cast(_) => {}
        }
    }
}

/// Collect every non-null cast column name under `nodes`, recursively (including
/// casts inside conditional blocks).
fn collect_casts(nodes: &[SqlToken], out: &mut HashSet<String>) {
    for node in nodes {
        match node {
            SqlToken::Cast(c) => {
                out.insert(c.name.clone());
            }
            SqlToken::Block(b) => collect_casts(&b.inner, out),
            SqlToken::Param(_) | SqlToken::Lit(_) => {}
        }
    }
}

/// Count `$` characters across all literal text, recursively (used for the
/// no-named-parameters fallback in [`SqlTree::param_names`]).
fn count_dollars(nodes: &[SqlToken]) -> usize {
    let mut n = 0;
    for node in nodes {
        match node {
            SqlToken::Lit(t) => n += t.matches('$').count(),
            SqlToken::Block(b) => n += count_dollars(&b.inner),
            SqlToken::Param(_) | SqlToken::Cast(_) => {}
        }
    }
    n
}

fn render_template(nodes: &[SqlToken], out: &mut String) {
    for node in nodes {
        match node {
            SqlToken::Lit(t) => out.push_str(t),
            SqlToken::Cast(c) => out.push_str(&c.name),
            SqlToken::Param(p) => push_named(&p.raw(), out),
            SqlToken::Block(b) => {
                out.push_str("#[");
                render_template(&b.inner, out);
                out.push(']');
            }
        }
    }
}

fn render_named_body(nodes: &[SqlToken], out: &mut String, inline_nested: bool) {
    for node in nodes {
        match node {
            SqlToken::Lit(t) => out.push_str(t),
            SqlToken::Cast(c) => out.push_str(&c.name),
            SqlToken::Param(p) => push_named(&p.raw(), out),
            SqlToken::Block(b) => {
                if inline_nested {
                    render_named_body(&b.inner, out, inline_nested);
                }
            }
        }
    }
}

fn render_positional_body(
    nodes: &[SqlToken],
    sql: &mut String,
    names: &mut Vec<String>,
    counter: &mut usize,
) {
    for node in nodes {
        match node {
            SqlToken::Lit(t) => sql.push_str(t),
            SqlToken::Cast(c) => sql.push_str(&c.name),
            SqlToken::Param(p) => emit_positional(&p.raw(), sql, names, counter),
            SqlToken::Block(b) => render_positional_body(&b.inner, sql, names, counter),
        }
    }
}

fn push_named(name: &str, out: &mut String) {
    out.push_str("#{");
    out.push_str(name);
    out.push('}');
}

fn emit_positional(name: &str, sql: &mut String, names: &mut Vec<String>, counter: &mut usize) {
    sql.push_str(&format!("${}", counter));
    *counter += 1;
    names.push(name.to_string());
}

/// Collapse runs of two spaces into one and trim, matching the whitespace
/// cleanup applied by `sqlfile_parser::remove_conditional_blocks`.
fn collapse_ws(sql: &str) -> String {
    sql.replace("  ", " ").trim().to_string()
}

/// Convert `#{name}` placeholders to `$N`, returning the positional SQL and the
/// ordered parameter names. Mirrors
/// `types_extractor::convert_named_params_to_positional` (including its "return
/// the input unchanged when there are no parameters" behavior).
fn to_positional(sql: &str) -> (String, Vec<String>) {
    let mut names = Vec::new();
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    let mut counter = 1;

    while let Some(ch) = chars.next() {
        if ch == '#' {
            if let Some(&'{') = chars.peek() {
                chars.next(); // consume '{'
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    name.push(c);
                }
                if !name.is_empty() {
                    names.push(name);
                    out.push_str(&format!("${}", counter));
                    counter += 1;
                }
            } else {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }

    if names.is_empty() {
        (sql.to_string(), Vec::new())
    } else {
        (out, names)
    }
}

// ===========================================================================
// Choice-group data types
//
// Derived, mutually-exclusive choice groups produced by
// `SqlTree::derive_choice_groups` and consumed by codegen. They live here (not
// in `query_definition`) so this module stays a dependency-free leaf.
// ===========================================================================

/// A single branch within a mutually-exclusive choice group.
///
/// Each branch corresponds to one conditional `#[...]` block that was tagged
/// with a selector directive `#{selector=variant}` / `#{selector=variant?}`.
#[derive(Debug, Clone)]
pub(crate) struct ChoiceVariant {
    /// Variant name from the selector directive (e.g. "ua_asc").
    pub variant: String,
    /// Indices, in source order, of the conditional block(s) that make up this
    /// branch. Most branches map to a single block, but a branch may span
    /// several blocks when the same `#{selector=variant}` directive is repeated
    /// (e.g. a projection fragment plus a matching `LEFT JOIN` fragment that
    /// must switch together).
    pub block_indices: Vec<usize>,
    /// Clean parameter names (suffixes stripped) referenced *directly* in this
    /// branch (outside any nested optional block), in source order, excluding the
    /// selector directive itself. These become mandatory (non-`Option`) enum
    /// variant fields.
    pub params: Vec<String>,
    /// Nested optional `#[...]` blocks declared inside this branch (Option B).
    /// Each nested block is included at runtime only when its gate parameter is
    /// `Some`, so its parameters become `Option<T>` enum variant fields. Empty
    /// for ordinary branches.
    pub nested_blocks: Vec<NestedChoiceBlock>,
}

impl ChoiceVariant {
    /// All clean parameter names referenced anywhere in this branch (direct
    /// fields first, then nested-block fields), in source order.
    pub fn all_params(&self) -> Vec<String> {
        let mut out = self.params.clone();
        for nb in &self.nested_blocks {
            out.extend(nb.params.iter().cloned());
        }
        out
    }
}

/// A nested optional conditional block declared inside a choice-group branch
/// (Option B keyset-pagination pattern). At runtime it is included only when its
/// gate parameter (`params[0]`) is `Some`, mirroring ordinary additive `#[...]`
/// blocks; its parameters therefore surface as `Option<T>` fields on the branch.
#[derive(Debug, Clone)]
pub(crate) struct NestedChoiceBlock {
    /// Clean parameter names (suffixes stripped) referenced in this nested block,
    /// in source order. `params[0]` is the include gate.
    pub params: Vec<String>,
}

/// A mutually-exclusive choice group: exactly one branch (required) or at most
/// one branch (`?`, optional) is selected at runtime via a generated enum
/// argument. Declared in SQL by prefixing each alternative conditional block
/// with `#{selector=variant}` (append `?` for an optional group).
#[derive(Debug, Clone)]
pub(crate) struct ChoiceGroup {
    /// Selector name (e.g. "sort") — becomes the generated enum argument name.
    pub selector: String,
    /// `true` if a branch must be chosen (no marker) → `selector: Enum`.
    /// `false` if the group is optional (`?` marker) → `selector: Option<Enum>`.
    pub required: bool,
    /// Branches in source order.
    pub variants: Vec<ChoiceVariant>,
}

// ===========================================================================
// Selector-directive parsing and Rust-identifier validation
//
// These small syntax helpers live here so the tree parser is self-contained;
// `sqlfile_parser` reuses them via `crate::sql_tree::*`.
// ===========================================================================

/// Parse a selector directive `#{selector=variant}` / `#{selector=variant?}` at
/// the very start (after optional whitespace) of a conditional block's content.
/// A plain directive (no marker) makes the group required; a `?` marker makes it
/// optional. Returns `(selector, variant, required, directive_char_len)` where
/// `directive_char_len` is the number of characters from the start of
/// `block_content` up to and including the closing `}` (so callers can strip it).
pub(crate) fn parse_selector_directive(
    block_content: &str,
) -> Option<(String, String, bool, usize)> {
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

    // Must be of the form: IDENT '=' IDENT ('?')? — a plain directive is required,
    // a trailing `?` makes the group optional. The `=` is what distinguishes a
    // selector directive from a plain parameter, so a marker-less directive never
    // collides with ordinary `#{name}` / `#{name?}`.
    let (body, required) = if let Some(stripped) = inner.strip_suffix('?') {
        (stripped, false)
    } else {
        (inner, true)
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

/// Check if a string is a valid Rust identifier
pub(crate) fn is_valid_rust_identifier(name: &str) -> bool {
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
pub(crate) fn is_rust_keyword(name: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_directive_defaults_to_required() {
        let (selector, variant, required, _len) =
            parse_selector_directive("#{referrer=on} r").expect("should parse without marker");
        assert_eq!(selector, "referrer");
        assert_eq!(variant, "on");
        assert!(required, "missing marker must default to required");
    }

    #[test]
    fn selector_directive_optional_marker() {
        let (_, _, required, _) =
            parse_selector_directive("#{sort=asc?} ORDER BY id").expect("`?` parses");
        assert!(!required);
    }

    #[test]
    fn selector_directive_bang_marker_not_supported() {
        // The `!` marker is no longer recognized: `asc!` is not a valid Rust
        // identifier, so the directive fails to parse and the block is treated
        // as an ordinary additive block instead.
        assert!(parse_selector_directive("#{sort=asc!} ORDER BY id").is_none());
    }

    #[test]
    fn plain_params_are_not_selector_directives() {
        // A plain optional param has no `=`, so it is never a selector directive.
        assert!(parse_selector_directive("#{category?} IS NULL").is_none());
        assert!(parse_selector_directive("#{name} = 'x'").is_none());
    }
}
