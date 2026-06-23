use walkdir::WalkDir;
use std::fs;
use syn::{Item, ImplItem, Meta, Expr, Lit};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let check_events = args.iter().any(|arg| arg == "--events" || arg == "-e");

    let mut missing = 0;
    for entry in WalkDir::new("../../onchain/contracts") {
        let entry = entry.unwrap();
        if entry.path().extension().map_or(false, |ext| ext == "rs") {
            let content = fs::read_to_string(entry.path()).unwrap();
            let file_name = entry.path().display().to_string();
            let missing_in_file = check_contract_docs(&content, &file_name, check_events);
            for msg in &missing_in_file {
                println!("{}", msg);
            }
            missing += missing_in_file.len();
        }
    }
    println!("Total instances missing some docs: {}", missing);
    if missing > 0 {
        std::process::exit(1);
    }
}

pub fn check_contract_docs(content: &str, file_name: &str, check_events: bool) -> Vec<String> {
    let mut errors = Vec::new();
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
                                let has_error = doc_lower.contains("error") || doc_lower.contains("err");
                                let has_access = doc_lower.contains("access") || doc_lower.contains("auth") || doc_lower.contains("require");
                                let has_docs = !doc_str.is_empty();
                                
                                let mut missing_parts = vec![];
                                if !has_docs {
                                    missing_parts.push("docs entirely missing");
                                } else {
                                    if !has_param { missing_parts.push("params"); }
                                    if !has_return { missing_parts.push("return"); }
                                    // Some functions don't return errors, checking if output contains Result
                                    if !has_access { missing_parts.push("access control"); }
                                }
                                
                                if !missing_parts.is_empty() {
                                    errors.push(format!("{}:{}: fn {} missing {}", file_name, func.sig.ident.span().start().line, func.sig.ident, missing_parts.join(", ")));
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
                                        errors.push(format!("{}:{}: struct {} missing docs for field {}", file_name, field.ident.as_ref().unwrap().span().start().line, item_struct.ident, field_name));
                                    }
                                }
                            } else if let syn::Fields::Unnamed(fields) = &item_struct.fields {
                                for (i, field) in fields.unnamed.iter().enumerate() {
                                    let has_docs = field.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                    if !has_docs {
                                        errors.push(format!("{}: struct {} missing docs for unnamed field {}", file_name, item_struct.ident, i));
                                    }
                                }
                            }
                        }
                    }
                },
                Item::Enum(item_enum) => {
                    if check_events {
                        let has_contracttype = item_enum.attrs.iter().any(|attr| attr.path().is_ident("contracttype"));
                        let ident_str = item_enum.ident.to_string();
                        let is_event_named = ident_str.contains("Event") || ident_str.contains("Payload");
                        
                        if has_contracttype && is_event_named {
                            for variant in &item_enum.variants {
                                let has_docs = variant.attrs.iter().any(|attr| attr.path().is_ident("doc"));
                                if !has_docs {
                                    errors.push(format!("{}:{}: enum {} missing docs for variant {}", file_name, variant.ident.span().start().line, item_enum.ident, variant.ident));
                                }
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
}
