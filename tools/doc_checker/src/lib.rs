use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use regex::Regex;
use syn::{Expr, ImplItem, Item, Lit, Meta};
use walkdir::WalkDir;

/// Whether a reported finding should fail the run or only warn.
///
/// New documentation rules (entirely-undocumented public functions,
/// undocumented error-enum variants, and orphaned `docs/*.md` files) default
/// to [`Severity::Warn`] so they can be rolled out incrementally without
/// immediately breaking CI. Passing `--strict` promotes every finding to
/// [`Severity::Fail`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Fail,
}

/// Runtime configuration for the documentation checker.
///
/// Each `check_*` flag turns on an additional category of checks, and
/// `*_severity` controls whether findings in the newer categories warn or fail.
#[derive(Clone, Copy, Debug)]
pub struct CheckConfig {
    /// Check documented fields/variants on event-like `#[contracttype]` items.
    pub check_events: bool,
    /// Flag public `#[contractimpl]` functions that have no doc comment at all.
    pub check_undocumented_fns: bool,
    /// Flag undocumented variants on `#[contracterror]` enums.
    pub check_error_enums: bool,
    /// Flag `docs/*.md` files unreachable from README.md / docs index files.
    pub check_orphaned_docs: bool,
    /// Flag backtick-quoted function references in docs that do not
    /// correspond to any public function in the contract tree.
    pub check_stale_refs: bool,
    /// Severity applied to the newer checks (undocumented fns / error
    /// variants / orphaned docs / stale refs).
    pub new_check_severity: Severity,
}

impl Default for CheckConfig {
    fn default() -> Self {
        CheckConfig {
            check_events: false,
            check_undocumented_fns: true,
            check_error_enums: true,
            check_orphaned_docs: true,
            check_stale_refs: true,
            new_check_severity: Severity::Warn,
        }
    }
}

/// A single documentation finding with its severity.
#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

/// Backwards-compatible wrapper returning only the finding messages.
///
/// Existing callers (and the original test suite) treat any reported item as an
/// error regardless of severity, so this collapses [`Finding`]s to plain strings.
pub fn check_contract_docs(content: &str, file_name: &str, check_events: bool) -> Vec<String> {
    let config = CheckConfig {
        check_events,
        new_check_severity: Severity::Fail,
        ..CheckConfig::default()
    };
    check_docs(content, file_name, &config)
        .into_iter()
        .map(|f| f.message)
        .collect()
}

/// Walks the parsed file and produces documentation [`Finding`]s per the config.
pub fn check_docs(content: &str, file_name: &str, config: &CheckConfig) -> Vec<Finding> {
    let mut errors: Vec<Finding> = Vec::new();
    let check_events = config.check_events;
    // Helper closures push at a fixed severity.
    macro_rules! fail {
        ($($arg:tt)*) => {
            errors.push(Finding { severity: Severity::Fail, message: format!($($arg)*) })
        };
    }
    macro_rules! new_finding {
        ($($arg:tt)*) => {
            errors.push(Finding { severity: config.new_check_severity, message: format!($($arg)*) })
        };
    }
    if let Ok(file) = syn::parse_file(content) {
        for item in &file.items {
            match item {
                Item::Impl(item_impl) => {
                    let has_contractimpl = item_impl
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("contractimpl"));
                    if !has_contractimpl {
                        continue;
                    }

                    for impl_item in &item_impl.items {
                        if let ImplItem::Fn(func) = impl_item {
                            if matches!(func.vis, syn::Visibility::Public(_)) {
                                let mut doc_str = String::new();
                                for attr in &func.attrs {
                                    if attr.path().is_ident("doc") {
                                        if let Meta::NameValue(nv) = &attr.meta {
                                            if let Expr::Lit(expr_lit) = &nv.value {
                                                if let Lit::Str(lit_str) = &expr_lit.lit {
                                                    doc_str.push_str(&lit_str.value());
                                                    doc_str.push('\n');
                                                }
                                            }
                                        }
                                    }
                                }
                                let doc_lower = doc_str.to_lowercase();
                                let has_param = doc_lower.contains("param")
                                    || doc_lower.contains("arguments")
                                    || func.sig.inputs.is_empty();
                                let has_return = doc_lower.contains("return")
                                    || matches!(func.sig.output, syn::ReturnType::Default);
                                let _has_error =
                                    doc_lower.contains("error") || doc_lower.contains("err");
                                let has_access = doc_lower.contains("access")
                                    || doc_lower.contains("auth")
                                    || doc_lower.contains("require");
                                let has_docs = !doc_str.is_empty();
                                let line = func.sig.ident.span().start().line;

                                if !has_docs {
                                    // Entirely-undocumented public function: a distinct,
                                    // configurable check so undocumented public surfaces
                                    // don't slip through as a single bundled message.
                                    if config.check_undocumented_fns {
                                        new_finding!(
                                            "{}:{}: fn {} has no doc comment at all",
                                            file_name,
                                            line,
                                            func.sig.ident
                                        );
                                    }
                                } else {
                                    let mut missing_parts = vec![];
                                    if !has_param {
                                        missing_parts.push("params");
                                    }
                                    if !has_return {
                                        missing_parts.push("return");
                                    }
                                    // Some functions don't return errors, checking if output contains Result
                                    if !has_access {
                                        missing_parts.push("access control");
                                    }
                                    if !missing_parts.is_empty() {
                                        fail!(
                                            "{}:{}: fn {} missing {}",
                                            file_name,
                                            line,
                                            func.sig.ident,
                                            missing_parts.join(", ")
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Item::Struct(item_struct) => {
                    if check_events {
                        let has_contracttype = item_struct
                            .attrs
                            .iter()
                            .any(|attr| attr.path().is_ident("contracttype"));
                        let ident_str = item_struct.ident.to_string();
                        let is_event_named =
                            ident_str.contains("Event") || ident_str.contains("Payload");

                        if has_contracttype && is_event_named {
                            if let syn::Fields::Named(fields) = &item_struct.fields {
                                for field in &fields.named {
                                    let has_docs =
                                        field.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                    if !has_docs {
                                        let field_name = field
                                            .ident
                                            .as_ref()
                                            .map(|i| i.to_string())
                                            .unwrap_or_else(|| "unnamed".to_string());
                                        fail!(
                                            "{}:{}: struct {} missing docs for field {}",
                                            file_name,
                                            field.ident.as_ref().unwrap().span().start().line,
                                            item_struct.ident,
                                            field_name
                                        );
                                    }
                                }
                            } else if let syn::Fields::Unnamed(fields) = &item_struct.fields {
                                for (i, field) in fields.unnamed.iter().enumerate() {
                                    let has_docs =
                                        field.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                    if !has_docs {
                                        fail!(
                                            "{}: struct {} missing docs for unnamed field {}",
                                            file_name,
                                            item_struct.ident,
                                            i
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Item::Enum(item_enum) => {
                    let ident_str = item_enum.ident.to_string();
                    let has_contracttype = item_enum
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("contracttype"));
                    let has_contracterror = item_enum
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("contracterror"));

                    if check_events {
                        let is_event_named =
                            ident_str.contains("Event") || ident_str.contains("Payload");

                        if has_contracttype && is_event_named {
                            for variant in &item_enum.variants {
                                let has_docs =
                                    variant.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                if !has_docs {
                                    fail!(
                                        "{}:{}: enum {} missing docs for variant {}",
                                        file_name,
                                        variant.ident.span().start().line,
                                        item_enum.ident,
                                        variant.ident
                                    );
                                }
                            }
                        }
                    }

                    // Error enums: every public error variant should be documented so
                    // the contract's failure modes are described. Detected via the
                    // `#[contracterror]` attribute (independent of the `--events` flag).
                    if config.check_error_enums && has_contracterror {
                        for variant in &item_enum.variants {
                            let has_docs =
                                variant.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                            if !has_docs {
                                new_finding!(
                                    "{}:{}: error enum {} variant {} has no doc comment",
                                    file_name,
                                    variant.ident.span().start().line,
                                    item_enum.ident,
                                    variant.ident
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    errors
}

// ============================================================================
// Orphaned documentation check
// ============================================================================
//
// Rationale: `docs/*.md` files that exist but are never linked from the
// repository README or any documentation index (`docs/README.md`,
// `docs/<section>/README.md`, ...) are effectively undiscoverable to a reader
// browsing the docs tree top-down. This check builds a reachability graph by
// following markdown links starting at those entry points and flags any
// `docs/*.md` file the traversal never visits.
//
// # Security notes
// - This check only *reads* files already committed to the repository; it
//   performs no network access and never treats a link target as a
//   command or path outside `repo_root` in a way that could be exploited —
//   targets are joined onto the referencing file's own directory using
//   ordinary path semantics, then resolved with `fs::canonicalize`, which
//   fails closed (the candidate is simply not counted as reachable) for
//   any path that does not exist on disk.
// - The traversal is cycle-safe: each canonical path is inserted into a
//   `visited` set before its own links are queued, so a documentation link
//   cycle (`a.md` -> `b.md` -> `a.md`) terminates instead of looping.

/// Extracts markdown link targets (`[text](target)`) from `content`.
///
/// This is a lightweight scan for the `](` delimiter rather than a full
/// markdown parser: documentation files in this repository do not need
/// anything more sophisticated, and a false-negative here only means a link
/// is not followed (the linked file may then be reported as orphaned when it
/// is not), which is safe to fail towards — it cannot hide a real problem.
fn extract_markdown_link_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut search_start = 0usize;
    while let Some(rel_pos) = content[search_start..].find("](") {
        let open_paren = search_start + rel_pos + 1;
        let target_start = open_paren + 1;
        match content[target_start..].find(')') {
            Some(rel_end) => {
                let target = content[target_start..target_start + rel_end].trim();
                if !target.is_empty() {
                    targets.push(target.to_string());
                }
                search_start = target_start + rel_end;
            }
            None => break,
        }
    }
    targets
}

/// Returns `true` for link targets that should not be followed on disk:
/// external URLs, mail links, and pure same-page anchors.
fn is_external_or_anchor_link(target: &str) -> bool {
    let t = target.trim();
    t.is_empty()
        || t.starts_with('#')
        || t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with("mailto:")
        || t.starts_with("file://")
        || t.starts_with("ftp://")
}

/// Resolves a markdown link target relative to `base_dir`, stripping any
/// `#fragment` suffix. Returns `None` for empty targets.
fn resolve_relative_link(base_dir: &Path, target: &str) -> Option<PathBuf> {
    let without_fragment = target.split('#').next().unwrap_or("").trim();
    if without_fragment.is_empty() {
        return None;
    }
    Some(base_dir.join(without_fragment))
}

/// Canonicalizes `path`, falling back to the original (non-canonical) path
/// when the target does not exist. A non-existent path can never match a
/// real `docs/*.md` file's canonical form, so this fallback cannot cause a
/// false "reachable" result — it only affects display/dedup of dead links.
fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Performs a breadth-first traversal of markdown links starting at
/// `entry_points`, returning the canonical paths of every `.md` file
/// reached (including the entry points themselves).
///
/// Non-existent files, non-markdown files, and already-visited files are
/// skipped without following their (non-existent) links further.
pub fn build_reachable_docs(entry_points: &[PathBuf]) -> HashSet<PathBuf> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<PathBuf> = entry_points.iter().cloned().collect();

    while let Some(candidate) = queue.pop_front() {
        let normalized = normalize_path(&candidate);
        if !normalized.is_file() {
            continue;
        }
        if !visited.insert(normalized.clone()) {
            // Already visited (or a duplicate entry point) - skip re-expanding.
            continue;
        }
        if normalized
            .extension()
            .map_or(true, |ext| !ext.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(&normalized) else {
            continue;
        };
        let base_dir = normalized
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        for target in extract_markdown_link_targets(&content) {
            if is_external_or_anchor_link(&target) {
                continue;
            }
            if let Some(resolved) = resolve_relative_link(&base_dir, &target) {
                queue.push_back(resolved);
            }
        }
    }

    visited
}

/// Recursively collects every `*.md` file under `docs_dir`.
pub fn find_all_docs_markdown(docs_dir: &Path) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = WalkDir::new(docs_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
        .map(|e| e.path().to_path_buf())
        .collect();
    result.sort();
    result
}

/// Returns every `docs/*.md` file under `repo_root/docs` that is not
/// reachable from `repo_root/README.md` or any `README.md` nested under
/// `docs/` (each such file is treated as a documentation index and used as
/// an additional traversal entry point, whether or not the top-level README
/// happens to link it).
///
/// Returns an empty list when `repo_root/docs` does not exist, since there
/// is nothing to check.
pub fn find_orphaned_docs(repo_root: &Path) -> Vec<PathBuf> {
    let docs_dir = repo_root.join("docs");
    if !docs_dir.is_dir() {
        return Vec::new();
    }

    let mut entry_points = vec![repo_root.join("README.md")];
    for entry in WalkDir::new(&docs_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_name() == "README.md" {
            entry_points.push(entry.path().to_path_buf());
        }
    }

    let reachable = build_reachable_docs(&entry_points);
    let all_docs = find_all_docs_markdown(&docs_dir);

    all_docs
        .into_iter()
        .filter(|doc| !reachable.contains(&normalize_path(doc)))
        .collect()
}

/// Produces [`Finding`]s for orphaned `docs/*.md` files, honoring
/// `config.check_orphaned_docs` and `config.new_check_severity`.
pub fn check_orphaned_docs(repo_root: &Path, config: &CheckConfig) -> Vec<Finding> {
    if !config.check_orphaned_docs {
        return Vec::new();
    }

    find_orphaned_docs(repo_root)
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(repo_root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            Finding {
                severity: config.new_check_severity,
                message: format!(
                    "{}: orphaned doc - not reachable from README.md or any docs index file",
                    relative
                ),
            }
        })
        .collect()
}

// ============================================================================
// Stale doc reference check
// ============================================================================
//
// Scans `docs/*.md` for backtick-quoted identifiers (e.g. `` `function_name` ``)
// that look like function names, then cross-references them against the set of
// `pub fn` names found in every `*.rs` file under the contracts tree. Any
// identifier found in the docs that does **not** exist as a public function
// anywhere in the contracts is reported as a stale reference.

/// Extract backtick-quoted snake_case identifiers from markdown content.
///
/// Returns a list of `(identifier, line_number)` tuples. Line numbers are
/// 1-indexed.
pub fn extract_backtick_references(content: &str) -> Vec<(String, usize)> {
    let re = Regex::new(r"`([a-z_][a-z0-9_]*)`").unwrap();
    let mut results = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        for cap in re.captures_iter(line) {
            let ident = cap[1].to_string();
            // Skip common non-function identifiers that appear in docs
            if ident == "no_std" || ident.starts_with("cfg_") {
                continue;
            }
            results.push((ident, line_idx + 1));
        }
    }
    results
}

/// Extract all public function names from a contract source file using regex.
///
/// Returns the list of function names found via `pub fn <name>` patterns.
pub fn extract_contract_functions(content: &str) -> Vec<String> {
    let re = Regex::new(r"pub\s+fn\s+(\w+)").unwrap();
    let mut fns = Vec::new();
    for cap in re.captures_iter(content) {
        let name = cap[1].to_string();
        if !fns.contains(&name) {
            fns.push(name);
        }
    }
    fns
}

/// Collect all public function names from all `*.rs` files under `contracts_dir`.
fn collect_contract_fns(contracts_dir: &Path) -> HashSet<String> {
    let mut all_fns: HashSet<String> = HashSet::new();
    let Ok(entries) = fs::read_dir(contracts_dir) else {
        return all_fns;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse into subdirectory
            let sub = collect_contract_fns(&path);
            all_fns.extend(sub);
        } else if path.extension().map_or(false, |ext| ext == "rs") {
            if let Ok(content) = fs::read_to_string(&path) {
                let fns = extract_contract_functions(&content);
                all_fns.extend(fns);
            }
        }
    }
    all_fns
}

/// Find stale backtick-quoted function references in documentation files.
///
/// Returns a list of tuples `(md_file_path, function_name, line_number)` for
/// every backtick reference that does **not** match any public function in the
/// contract tree.
pub fn find_stale_references(
    docs_dir: &Path,
    contracts_dir: &Path,
) -> Vec<(PathBuf, String, usize)> {
    let known_fns = collect_contract_fns(contracts_dir);
    let mut stale: Vec<(PathBuf, String, usize)> = Vec::new();

    let entries: Vec<_> = WalkDir::new(docs_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
        .collect();

    for entry in entries {
        let path = entry.path().to_path_buf();
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for (ident, line) in extract_backtick_references(&content) {
            if !known_fns.contains(&ident) {
                stale.push((path.clone(), ident, line));
            }
        }
    }
    stale
}

/// Produces [`Finding`]s for stale doc references, honoring
/// `config.check_stale_refs` and `config.new_check_severity`.
pub fn check_stale_doc_references(repo_root: &Path, config: &CheckConfig) -> Vec<Finding> {
    if !config.check_stale_refs {
        return Vec::new();
    }

    let docs_dir = repo_root.join("docs");
    let contracts_dir = repo_root.join("contracts");

    if !docs_dir.is_dir() || !contracts_dir.is_dir() {
        return Vec::new();
    }

    find_stale_references(&docs_dir, &contracts_dir)
        .into_iter()
        .map(|(path, func_name, line)| {
            let relative = path
                .strip_prefix(repo_root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            Finding {
                severity: config.new_check_severity,
                message: format!(
                    "{}:{}: stale doc reference to `{}` – no matching pub fn found in contracts",
                    relative, line, func_name
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_documented_event_passes() {
        let code = r#"
        #[contracttype]
        pub struct TransferEvent {
            /// The sender.
            pub from: Address,
            /// The recipient.
            pub to: Address,
        }
        "#;
        let errors = check_contract_docs(code, "test.rs", true);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_undocumented_event_fails() {
        let code = r#"
        #[contracttype]
        pub struct TransferEvent {
            pub from: Address, // missing docs
            /// receiver
            pub to: Address,
        }
        "#;
        let errors = check_contract_docs(code, "test.rs", true);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing docs for field from"));
    }

    #[test]
    fn test_non_event_struct_ignored() {
        let code = r#"
        #[contracttype]
        pub struct StateStruct {
            pub from: Address,
        }
        "#;
        let errors = check_contract_docs(code, "test.rs", true);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_events_flag_disabled_ignores_undocumented_events() {
        let code = r#"
        #[contracttype]
        pub struct TransferEvent {
            pub from: Address,
        }
        "#;
        let errors = check_contract_docs(code, "test.rs", false);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_enum_event_fails() {
        let code = r#"
        #[contracttype]
        pub enum OpPayload {
            /// Variant doc
            A,
            B, // missing doc
        }
        "#;
        let errors = check_contract_docs(code, "test.rs", true);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing docs for variant B"));
    }

    fn warn_config() -> CheckConfig {
        CheckConfig {
            check_events: false,
            check_undocumented_fns: true,
            check_error_enums: true,
            check_orphaned_docs: false,
            check_stale_refs: false,
            new_check_severity: Severity::Warn,
        }
    }

    #[test]
    fn test_undocumented_fn_flagged() {
        let code = r#"
        #[contractimpl]
        impl C {
            pub fn no_docs(env: Env, x: i128) -> i128 { x }
        }
        "#;
        let findings = check_docs(code, "test.rs", &warn_config());
        assert_eq!(findings.len(), 1, "got: {:?}", findings);
        assert!(findings[0].message.contains("no doc comment at all"));
        assert_eq!(findings[0].severity, Severity::Warn);
    }

    #[test]
    fn test_partial_doc_fn_not_reported_as_undocumented() {
        // A function with some docs is handled by the existing section-based
        // check, not the "no doc comment at all" rule.
        let code = r#"
        #[contractimpl]
        impl C {
            /// Does a thing.
            pub fn partial(env: Env, x: i128) -> i128 { x }
        }
        "#;
        let findings = check_docs(code, "test.rs", &warn_config());
        assert!(findings
            .iter()
            .all(|f| !f.message.contains("no doc comment at all")));
    }

    #[test]
    fn test_undocumented_error_variant_flagged() {
        let code = r#"
        #[contracterror]
        pub enum MyError {
            /// Documented.
            First = 1,
            Second = 2,
        }
        "#;
        let findings = check_docs(code, "test.rs", &warn_config());
        assert_eq!(findings.len(), 1, "got: {:?}", findings);
        assert!(findings[0]
            .message
            .contains("variant Second has no doc comment"));
    }

    #[test]
    fn test_documented_error_enum_passes() {
        let code = r#"
        #[contracterror]
        pub enum MyError {
            /// First.
            First = 1,
            /// Second.
            Second = 2,
        }
        "#;
        let findings = check_docs(code, "test.rs", &warn_config());
        assert!(findings.is_empty(), "got: {:?}", findings);
    }

    #[test]
    fn test_strict_promotes_new_checks_to_fail() {
        let code = r#"
        #[contracterror]
        pub enum MyError {
            Undocumented = 1,
        }
        "#;
        let mut cfg = warn_config();
        cfg.new_check_severity = Severity::Fail;
        let findings = check_docs(code, "test.rs", &cfg);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Fail);
    }

    #[test]
    fn test_new_checks_can_be_disabled() {
        let code = r#"
        #[contracterror]
        pub enum MyError {
            Undocumented = 1,
        }
        #[contractimpl]
        impl C {
            pub fn no_docs(env: Env) {}
        }
        "#;
        let cfg = CheckConfig {
            check_events: false,
            check_undocumented_fns: false,
            check_error_enums: false,
            check_orphaned_docs: false,
            check_stale_refs: false,
            new_check_severity: Severity::Warn,
        };
        let findings = check_docs(code, "test.rs", &cfg);
        assert!(findings.is_empty(), "got: {:?}", findings);
    }

    #[test]
    fn test_malformed_source_does_not_panic() {
        // Fails safe: unparseable input yields no findings rather than crashing.
        let code = "this is not valid rust ;;; {{{";
        let findings = check_docs(code, "test.rs", &warn_config());
        assert!(findings.is_empty());
    }

    // ------------------------------------------------------------------
    // Orphaned docs: link-extraction unit tests
    // ------------------------------------------------------------------

    #[test]
    fn test_extract_markdown_link_targets_basic() {
        let content = "See [Architecture](./architecture.md) and [API](api/README.md).";
        let targets = extract_markdown_link_targets(content);
        assert_eq!(targets, vec!["./architecture.md", "api/README.md"]);
    }

    #[test]
    fn test_extract_markdown_link_targets_ignores_unclosed() {
        // A dangling "](" with no closing paren must not panic or hang.
        let content = "broken link [text](";
        let targets = extract_markdown_link_targets(content);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_is_external_or_anchor_link() {
        assert!(is_external_or_anchor_link("#section"));
        assert!(is_external_or_anchor_link("https://example.com"));
        assert!(is_external_or_anchor_link("http://example.com"));
        assert!(is_external_or_anchor_link("mailto:a@b.com"));
        assert!(is_external_or_anchor_link("file:///abs/path.md"));
        assert!(is_external_or_anchor_link(""));
        assert!(!is_external_or_anchor_link("./relative.md"));
        assert!(!is_external_or_anchor_link("../up/relative.md"));
    }

    #[test]
    fn test_resolve_relative_link_strips_fragment() {
        let base = Path::new("/repo/docs");
        let resolved = resolve_relative_link(base, "architecture.md#overview").unwrap();
        assert_eq!(resolved, PathBuf::from("/repo/docs/architecture.md"));
    }

    #[test]
    fn test_resolve_relative_link_empty_target_is_none() {
        let base = Path::new("/repo/docs");
        assert!(resolve_relative_link(base, "#only-anchor").is_none());
        assert!(resolve_relative_link(base, "").is_none());
    }

    // ------------------------------------------------------------------
    // Stale doc reference checks: unit tests
    // ------------------------------------------------------------------

    #[test]
    fn test_extract_backtick_references_basic() {
        let md = "Call `initialize` and then `get_status`.";
        let refs = extract_backtick_references(md);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], ("initialize".to_string(), 1));
        assert_eq!(refs[1], ("get_status".to_string(), 1));
    }

    #[test]
    fn test_extract_backtick_references_multiline() {
        let md = "First, call `initialize`.\nThen call `check_and_consume`.";
        let refs = extract_backtick_references(md);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], ("initialize".to_string(), 1));
        assert_eq!(refs[1], ("check_and_consume".to_string(), 2));
    }

    #[test]
    fn test_extract_backtick_references_skips_no_std() {
        let md = "Uses `#![no_std]` and `initialize`.";
        let refs = extract_backtick_references(md);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, "initialize");
    }

    #[test]
    fn test_extract_backtick_references_empty() {
        let md = "No code references here.";
        let refs = extract_backtick_references(md);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_contract_functions_basic() {
        let code = r#"
        pub fn initialize(env: Env) {}
        pub fn get_status(env: Env) -> u32 { 0 }
        fn internal_helper() {}
        "#;
        let fns = extract_contract_functions(code);
        assert_eq!(fns.len(), 2);
        assert!(fns.contains(&"initialize".to_string()));
        assert!(fns.contains(&"get_status".to_string()));
    }

    #[test]
    fn test_extract_contract_functions_dedup() {
        let code = r#"
        pub fn initialize(env: Env) {}
        pub fn initialize(env: Env, extra: i128) {}
        "#;
        let fns = extract_contract_functions(code);
        assert_eq!(fns.len(), 1);
    }

    #[test]
    fn test_collect_contract_fns_empty_dir() {
        let dir = Path::new("/nonexistent/path");
        let fns = collect_contract_fns(dir);
        assert!(fns.is_empty());
    }

    #[test]
    fn test_find_stale_references_empty_docs() {
        let docs = Path::new("/nonexistent/docs");
        let contracts = Path::new("/nonexistent/contracts");
        let stale = find_stale_references(docs, contracts);
        assert!(stale.is_empty());
    }

    #[test]
    fn test_check_stale_doc_references_disabled() {
        let config = CheckConfig {
            check_stale_refs: false,
            ..CheckConfig::default()
        };
        let repo_root = Path::new("/tmp");
        let findings = check_stale_doc_references(repo_root, &config);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_check_stale_doc_references_no_docs_dir() {
        let config = CheckConfig::default();
        let repo_root = Path::new("/nonexistent");
        let findings = check_stale_doc_references(repo_root, &config);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_extract_backtick_references_only_snake_case() {
        let md = "Use `CamelCase` or `snake_case` but only the latter is extracted.";
        let refs = extract_backtick_references(md);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, "snake_case");
    }
}
