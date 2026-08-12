use std::path::Path;

use ast_grep_core::MatchStrictness;
use ast_grep_language::{Language, SupportLang};

use crate::ast::AstError;

pub type LanguageInfo = (String, Vec<String>, Vec<String>);

pub fn supported_languages() -> Vec<LanguageInfo> {
    SupportLang::all_langs()
        .iter()
        .copied()
        .map(|language| {
            let (canonical, aliases, extensions) = metadata(language);
            (
                canonical.to_owned(),
                aliases.iter().map(|value| (*value).to_owned()).collect(),
                extensions.iter().map(|value| (*value).to_owned()).collect(),
            )
        })
        .collect()
}

pub fn resolve_language(value: &str) -> Result<SupportLang, AstError> {
    value
        .parse()
        .map_err(|_| AstError::Language(format!("unsupported AST language: {value}")))
}

pub fn infer_language(path: &Path) -> Option<SupportLang> {
    SupportLang::from_path(path)
}

pub fn canonical_name(language: SupportLang) -> &'static str {
    metadata(language).0
}

pub fn strictness(value: &str) -> Result<MatchStrictness, AstError> {
    match value {
        "cst" => Ok(MatchStrictness::Cst),
        "smart" => Ok(MatchStrictness::Smart),
        "ast" => Ok(MatchStrictness::Ast),
        "relaxed" => Ok(MatchStrictness::Relaxed),
        "signature" => Ok(MatchStrictness::Signature),
        "template" => Ok(MatchStrictness::Template),
        _ => Err(AstError::Configuration(format!(
            "unsupported AST strictness: {value}"
        ))),
    }
}

fn metadata(
    language: SupportLang,
) -> (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
) {
    use SupportLang as L;

    match language {
        L::Bash => (
            "bash",
            &["bash"],
            &[
                "bash", "bats", "cgi", "command", "env", "fcgi", "ksh", "sh", "tmux", "tool", "zsh",
            ],
        ),
        L::C => ("c", &["c"], &["c", "h"]),
        L::Cpp => (
            "cpp",
            &["cc", "c++", "cpp", "cxx"],
            &["cc", "hpp", "cpp", "c++", "hh", "cxx", "cu", "ino"],
        ),
        L::CSharp => ("csharp", &["cs", "csharp"], &["cs"]),
        L::Css => ("css", &["css"], &["css", "scss"]),
        L::Dart => ("dart", &["dart"], &["dart"]),
        L::Elixir => ("elixir", &["ex", "elixir"], &["ex", "exs"]),
        L::Go => ("go", &["go", "golang"], &["go"]),
        L::Haskell => ("haskell", &["hs", "haskell"], &["hs"]),
        L::Hcl => (
            "hcl",
            &["hcl"],
            &["hcl", "nomad", "tf", "tfvars", "workflow"],
        ),
        L::Html => ("html", &["html"], &["html", "htm", "xhtml"]),
        L::Java => ("java", &["java"], &["java"]),
        L::JavaScript => (
            "javascript",
            &["javascript", "js", "jsx"],
            &["cjs", "js", "mjs", "jsx"],
        ),
        L::Json => ("json", &["json"], &["json"]),
        L::Kotlin => ("kotlin", &["kotlin", "kt"], &["kt", "ktm", "kts"]),
        L::Lua => ("lua", &["lua"], &["lua"]),
        L::Markdown => ("markdown", &["markdown", "md"], &["markdown", "md"]),
        L::Nix => ("nix", &["nix"], &["nix"]),
        L::Php => ("php", &["php"], &["php"]),
        L::Python => (
            "python",
            &["py", "python"],
            &["py", "py3", "pyi", "bzl", "bazel"],
        ),
        L::Ruby => ("ruby", &["rb", "ruby"], &["rb", "rbw", "gemspec"]),
        L::Rust => ("rust", &["rs", "rust"], &["rs"]),
        L::Scala => ("scala", &["scala"], &["scala", "sc", "sbt"]),
        L::Solidity => ("solidity", &["sol", "solidity"], &["sol"]),
        L::Swift => ("swift", &["swift"], &["swift"]),
        L::Tsx => ("tsx", &["tsx"], &["tsx"]),
        L::TypeScript => ("typescript", &["ts", "typescript"], &["ts", "cts", "mts"]),
        L::Yaml => ("yaml", &["yaml", "yml"], &["yaml", "yml"]),
    }
}
