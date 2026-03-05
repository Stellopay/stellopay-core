use walkdir::WalkDir;
use std::fs;
use syn::{Item, ImplItem, Meta, Expr, Lit};

fn main() {
    let mut missing = 0;
    for entry in WalkDir::new("../../onchain/contracts") {
        let entry = entry.unwrap();
        if entry.path().extension().map_or(false, |ext| ext == "rs") {
            let content = fs::read_to_string(entry.path()).unwrap();
            if let Ok(file) = syn::parse_file(&content) {
                for item in file.items {
                    if let Item::Impl(item_impl) = item {
                        let has_contractimpl = item_impl.attrs.iter().any(|attr| {
                            attr.path().is_ident("contractimpl")
                        });
                        if !has_contractimpl { continue; }
                        
                        for impl_item in item_impl.items {
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
                                        println!("{}:{}: fn {} missing {}", entry.path().display(), func.sig.ident.span().start().line, func.sig.ident, missing_parts.join(", "));
                                        missing += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    println!("Total functions missing some docs: {}", missing);
}
