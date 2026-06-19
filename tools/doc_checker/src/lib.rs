use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use syn::{Expr, ImplItem, Item, Lit, Meta};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Severity {
    Warn,
    Fail,
}

impl Severity {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "warn" => Some(Self::Warn),
            "fail" => Some(Self::Fail),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Warn => "warning",
            Self::Fail => "error",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Config {
    pub contracts_dir: PathBuf,
    pub severity: Severity,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            contracts_dir: PathBuf::from("../../onchain/contracts"),
            severity: Severity::Warn,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CliAction {
    Run(Config),
    Help,
}

impl Config {
    pub fn from_args<I, S>(args: I) -> Result<CliAction, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut config = Self::default();
        let mut args = args.into_iter().map(Into::into);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(CliAction::Help),
                "--contracts-dir" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--contracts-dir requires a path".to_string())?;
                    config.contracts_dir = PathBuf::from(value);
                }
                "--severity" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--severity requires warn or fail".to_string())?;
                    config.severity = Severity::parse(&value)
                        .ok_or_else(|| "--severity must be warn or fail".to_string())?;
                }
                "--fail" => {
                    config.severity = Severity::Fail;
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }

        Ok(CliAction::Run(config))
    }

    pub fn usage() -> &'static str {
        "Usage: doc_checker [--contracts-dir PATH] [--severity warn|fail] [--fail]\n\
         \n\
         Defaults to scanning ../../onchain/contracts and reporting warnings.\n\
         Use --severity fail or --fail to exit with status 1 when findings exist."
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FindingKind {
    ContractFunctionMissingDocs,
    ContractFunctionIncompleteDocs,
    ErrorVariantMissingDocs,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Finding {
    pub path: PathBuf,
    pub line: usize,
    pub kind: FindingKind,
    pub message: String,
}

impl Finding {
    fn new(path: &Path, line: usize, kind: FindingKind, message: String) -> Self {
        Self {
            path: path.to_path_buf(),
            line,
            kind,
            message,
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.path.display(), self.line, self.message)
    }
}

pub fn scan_contracts_dir(root: &Path) -> Result<Vec<Finding>, Box<dyn std::error::Error>> {
    let mut findings = Vec::new();

    for entry in WalkDir::new(root) {
        let entry = entry?;
        if !entry.path().extension().is_some_and(|ext| ext == "rs") {
            continue;
        }

        let content = fs::read_to_string(entry.path())?;
        if let Ok(file) = syn::parse_file(&content) {
            findings.extend(scan_file(entry.path(), file));
        }
    }

    Ok(findings)
}

pub fn scan_source(path: impl AsRef<Path>, source: &str) -> Result<Vec<Finding>, syn::Error> {
    let file = syn::parse_file(source)?;
    Ok(scan_file(path.as_ref(), file))
}

fn scan_file(path: &Path, file: syn::File) -> Vec<Finding> {
    let mut findings = Vec::new();

    for item in file.items {
        match item {
            Item::Impl(item_impl) if has_attr(&item_impl.attrs, "contractimpl") => {
                for impl_item in item_impl.items {
                    if let ImplItem::Fn(func) = impl_item {
                        if !matches!(func.vis, syn::Visibility::Public(_)) {
                            continue;
                        }

                        let docs = doc_text(&func.attrs);
                        let mut missing_parts = Vec::new();
                        if docs.trim().is_empty() {
                            findings.push(Finding::new(
                                path,
                                func.sig.ident.span().start().line,
                                FindingKind::ContractFunctionMissingDocs,
                                format!(
                                    "public #[contractimpl] fn {} is missing doc comments",
                                    func.sig.ident
                                ),
                            ));
                            continue;
                        }

                        let docs_lower = docs.to_lowercase();
                        if !docs_lower.contains("param")
                            && !docs_lower.contains("arguments")
                            && !func.sig.inputs.is_empty()
                        {
                            missing_parts.push("params");
                        }
                        if !docs_lower.contains("return")
                            && !matches!(func.sig.output, syn::ReturnType::Default)
                        {
                            missing_parts.push("return");
                        }
                        if !docs_lower.contains("access")
                            && !docs_lower.contains("auth")
                            && !docs_lower.contains("require")
                        {
                            missing_parts.push("access control");
                        }

                        if !missing_parts.is_empty() {
                            findings.push(Finding::new(
                                path,
                                func.sig.ident.span().start().line,
                                FindingKind::ContractFunctionIncompleteDocs,
                                format!(
                                    "public #[contractimpl] fn {} doc comments missing {}",
                                    func.sig.ident,
                                    missing_parts.join(", ")
                                ),
                            ));
                        }
                    }
                }
            }
            Item::Enum(item_enum)
                if matches!(item_enum.vis, syn::Visibility::Public(_))
                    && has_attr(&item_enum.attrs, "contracterror") =>
            {
                for variant in item_enum.variants {
                    if doc_text(&variant.attrs).trim().is_empty() {
                        findings.push(Finding::new(
                            path,
                            variant.ident.span().start().line,
                            FindingKind::ErrorVariantMissingDocs,
                            format!(
                                "public #[contracterror] variant {}::{} is missing doc comments",
                                item_enum.ident, variant.ident
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    findings
}

fn has_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == name)
    })
}

fn doc_text(attrs: &[syn::Attribute]) -> String {
    let mut docs = String::new();

    for attr in attrs {
        if !has_attr(std::slice::from_ref(attr), "doc") {
            continue;
        }

        if let Meta::NameValue(name_value) = &attr.meta {
            if let Expr::Lit(expr_lit) = &name_value.value {
                if let Lit::Str(lit_str) = &expr_lit.lit {
                    docs.push_str(&lit_str.value());
                    docs.push('\n');
                }
            }
        }
    }

    docs
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/contract_docs.rs");

    #[test]
    fn detects_public_contractimpl_functions_without_doc_comments() {
        let findings = scan_source("fixture.rs", FIXTURE).unwrap();

        assert!(findings.iter().any(|finding| {
            finding.kind == FindingKind::ContractFunctionMissingDocs
                && finding.message.contains("missing_contract_docs")
        }));
    }

    #[test]
    fn keeps_existing_partial_doc_checks_for_contractimpl_functions() {
        let findings = scan_source("fixture.rs", FIXTURE).unwrap();

        assert!(findings.iter().any(|finding| {
            finding.kind == FindingKind::ContractFunctionIncompleteDocs
                && finding.message.contains("missing_contract_sections")
                && finding.message.contains("access control")
        }));
    }

    #[test]
    fn detects_public_contracterror_variants_without_doc_comments() {
        let findings = scan_source("fixture.rs", FIXTURE).unwrap();

        assert!(findings.iter().any(|finding| {
            finding.kind == FindingKind::ErrorVariantMissingDocs
                && finding.message.contains("ExampleError::MissingDocs")
        }));
    }

    #[test]
    fn ignores_private_items_and_documented_public_items() {
        let findings = scan_source("fixture.rs", FIXTURE).unwrap();

        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("private_missing_docs")));
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("documented_contract_fn")));
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("PrivateError::MissingDocs")));
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("ExampleError::Documented")));
    }

    #[test]
    fn parses_cli_severity_configuration() {
        let action =
            Config::from_args(["--contracts-dir", "contracts", "--severity", "fail"]).unwrap();

        assert_eq!(
            action,
            CliAction::Run(Config {
                contracts_dir: PathBuf::from("contracts"),
                severity: Severity::Fail,
            })
        );
        assert_eq!(
            Config::from_args(["--fail"]).unwrap(),
            CliAction::Run(Config {
                contracts_dir: PathBuf::from("../../onchain/contracts"),
                severity: Severity::Fail,
            })
        );
    }
}
