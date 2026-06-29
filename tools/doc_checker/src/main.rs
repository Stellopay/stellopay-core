use walkdir::WalkDir;
use std::fs;
use syn::{Item, ImplItem, Meta, Expr, Lit};

/// Whether a reported finding should fail the run or only warn.
///
/// New documentation rules (entirely-undocumented public functions and
/// undocumented error-enum variants) default to [`Severity::Warn`] so they can
/// be rolled out incrementally without immediately breaking CI. Passing
/// `--strict` promotes every finding to [`Severity::Fail`].
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
    /// Severity applied to the two newer checks (undocumented fns / error variants).
    pub new_check_severity: Severity,
}

impl Default for CheckConfig {
    fn default() -> Self {
        CheckConfig {
            check_events: false,
            check_undocumented_fns: true,
            check_error_enums: true,
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut config = CheckConfig::default();
    config.check_events = args.iter().any(|arg| arg == "--events" || arg == "-e");
    if args.iter().any(|arg| arg == "--strict") {
        config.new_check_severity = Severity::Fail;
    }
    if args.iter().any(|arg| arg == "--no-undocumented-fns") {
        config.check_undocumented_fns = false;
    }
    if args.iter().any(|arg| arg == "--no-error-enums") {
        config.check_error_enums = false;
    }

    let mut warnings = 0;
    let mut failures = 0;
    for entry in WalkDir::new("../../onchain/contracts") {
        let entry = entry.unwrap();
        if entry.path().extension().map_or(false, |ext| ext == "rs") {
            let content = fs::read_to_string(entry.path()).unwrap();
            let file_name = entry.path().display().to_string();
            let findings = check_docs(&content, &file_name, &config);
            for finding in &findings {
                let label = match finding.severity {
                    Severity::Warn => "warning",
                    Severity::Fail => "error",
                };
                println!("{}: {}", label, finding.message);
                match finding.severity {
                    Severity::Warn => warnings += 1,
                    Severity::Fail => failures += 1,
                }
            }
        }
    }
    println!(
        "Documentation findings: {} error(s), {} warning(s)",
        failures, warnings
    );
    if failures > 0 {
        std::process::exit(1);
    }
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
                    let has_contractimpl = item_impl.attrs.iter().any(|attr| {
                        attr.path().is_ident("contractimpl")
                    });
                    if !has_contractimpl { continue; }
                    
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
                                let has_param = doc_lower.contains("param") || doc_lower.contains("arguments") || func.sig.inputs.is_empty();
                                let has_return = doc_lower.contains("return") || matches!(func.sig.output, syn::ReturnType::Default);
                                let _has_error = doc_lower.contains("error") || doc_lower.contains("err");
                                let has_access = doc_lower.contains("access") || doc_lower.contains("auth") || doc_lower.contains("require");
                                let has_docs = !doc_str.is_empty();
                                let line = func.sig.ident.span().start().line;

                                if !has_docs {
                                    // Entirely-undocumented public function: a distinct,
                                    // configurable check so undocumented public surfaces
                                    // don't slip through as a single bundled message.
                                    if config.check_undocumented_fns {
                                        new_finding!(
                                            "{}:{}: fn {} has no doc comment at all",
                                            file_name, line, func.sig.ident
                                        );
                                    }
                                } else {
                                    let mut missing_parts = vec![];
                                    if !has_param { missing_parts.push("params"); }
                                    if !has_return { missing_parts.push("return"); }
                                    // Some functions don't return errors, checking if output contains Result
                                    if !has_access { missing_parts.push("access control"); }
                                    if !missing_parts.is_empty() {
                                        fail!(
                                            "{}:{}: fn {} missing {}",
                                            file_name, line, func.sig.ident, missing_parts.join(", ")
                                        );
                                    }
                                }
                            }
                        }
                    }
                },
                Item::Struct(item_struct) => {
                    if check_events {
                        let has_contracttype = item_struct.attrs.iter().any(|attr| attr.path().is_ident("contracttype"));
                        let ident_str = item_struct.ident.to_string();
                        let is_event_named = ident_str.contains("Event") || ident_str.contains("Payload");
                        
                        if has_contracttype && is_event_named {
                            if let syn::Fields::Named(fields) = &item_struct.fields {
                                for field in &fields.named {
                                    let has_docs = field.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                    if !has_docs {
                                        let field_name = field.ident.as_ref().map(|i| i.to_string()).unwrap_or_else(|| "unnamed".to_string());
                                        fail!("{}:{}: struct {} missing docs for field {}", file_name, field.ident.as_ref().unwrap().span().start().line, item_struct.ident, field_name);
                                    }
                                }
                            } else if let syn::Fields::Unnamed(fields) = &item_struct.fields {
                                for (i, field) in fields.unnamed.iter().enumerate() {
                                    let has_docs = field.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                    if !has_docs {
                                        fail!("{}: struct {} missing docs for unnamed field {}", file_name, item_struct.ident, i);
                                    }
                                }
                            }
                        }
                    }
                },
                Item::Enum(item_enum) => {
                    let ident_str = item_enum.ident.to_string();
                    let has_contracttype = item_enum.attrs.iter().any(|attr| attr.path().is_ident("contracttype"));
                    let has_contracterror = item_enum.attrs.iter().any(|attr| attr.path().is_ident("contracterror"));

                    if check_events {
                        let is_event_named = ident_str.contains("Event") || ident_str.contains("Payload");

                        if has_contracttype && is_event_named {
                            for variant in &item_enum.variants {
                                let has_docs = variant.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                if !has_docs {
                                    fail!("{}:{}: enum {} missing docs for variant {}", file_name, variant.ident.span().start().line, item_enum.ident, variant.ident);
                                }
                            }
                        }
                    }

                    // Error enums: every public error variant should be documented so
                    // the contract's failure modes are described. Detected via the
                    // `#[contracterror]` attribute (independent of the `--events` flag).
                    if config.check_error_enums && has_contracterror {
                        for variant in &item_enum.variants {
                            let has_docs = variant.attrs.iter().any(|attr| attr.path().is_ident("doc"));
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
                },
                _ => {}
            }
        }
    }
    errors
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
        assert!(findings.iter().all(|f| !f.message.contains("no doc comment at all")));
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
        assert!(findings[0].message.contains("variant Second has no doc comment"));
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
}
