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
            let (canonical, aliases, extensions) = METADATA[language as usize];
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
    METADATA[language as usize].0
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

type LanguageMetadata = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

const METADATA: [LanguageMetadata; SupportLang::all_langs().len()] = {
    let entries: &[(SupportLang, LanguageMetadata)] = &[
        (
            SupportLang::Bash,
            (
                "bash",
                &["bash"],
                &[
                    "bash", "bats", "cgi", "command", "env", "fcgi", "ksh", "sh", "tmux", "tool",
                    "zsh",
                ],
            ),
        ),
        (SupportLang::C, ("c", &["c"], &["c", "h"])),
        (
            SupportLang::Cpp,
            (
                "cpp",
                &["cc", "c++", "cpp", "cxx"],
                &["cc", "hpp", "cpp", "c++", "hh", "cxx", "cu", "ino"],
            ),
        ),
        (SupportLang::CSharp, ("csharp", &["cs", "csharp"], &["cs"])),
        (SupportLang::Css, ("css", &["css"], &["css", "scss"])),
        (SupportLang::Dart, ("dart", &["dart"], &["dart"])),
        (
            SupportLang::Elixir,
            ("elixir", &["ex", "elixir"], &["ex", "exs"]),
        ),
        (SupportLang::Go, ("go", &["go", "golang"], &["go"])),
        (
            SupportLang::Haskell,
            ("haskell", &["hs", "haskell"], &["hs"]),
        ),
        (
            SupportLang::Hcl,
            (
                "hcl",
                &["hcl"],
                &["hcl", "nomad", "tf", "tfvars", "workflow"],
            ),
        ),
        (
            SupportLang::Html,
            ("html", &["html"], &["html", "htm", "xhtml"]),
        ),
        (SupportLang::Java, ("java", &["java"], &["java"])),
        (
            SupportLang::JavaScript,
            (
                "javascript",
                &["javascript", "js", "jsx"],
                &["cjs", "js", "mjs", "jsx"],
            ),
        ),
        (SupportLang::Json, ("json", &["json"], &["json"])),
        (
            SupportLang::Kotlin,
            ("kotlin", &["kotlin", "kt"], &["kt", "ktm", "kts"]),
        ),
        (SupportLang::Lua, ("lua", &["lua"], &["lua"])),
        (
            SupportLang::Markdown,
            ("markdown", &["markdown", "md"], &["markdown", "md"]),
        ),
        (SupportLang::Nix, ("nix", &["nix"], &["nix"])),
        (SupportLang::Php, ("php", &["php"], &["php"])),
        (
            SupportLang::Python,
            (
                "python",
                &["py", "python"],
                &["py", "py3", "pyi", "bzl", "bazel"],
            ),
        ),
        (
            SupportLang::Ruby,
            ("ruby", &["rb", "ruby"], &["rb", "rbw", "gemspec"]),
        ),
        (SupportLang::Rust, ("rust", &["rs", "rust"], &["rs"])),
        (
            SupportLang::Scala,
            ("scala", &["scala"], &["scala", "sc", "sbt"]),
        ),
        (
            SupportLang::Solidity,
            ("solidity", &["sol", "solidity"], &["sol"]),
        ),
        (SupportLang::Swift, ("swift", &["swift"], &["swift"])),
        (SupportLang::Tsx, ("tsx", &["tsx"], &["tsx"])),
        (
            SupportLang::TypeScript,
            ("typescript", &["ts", "typescript"], &["ts", "cts", "mts"]),
        ),
        (
            SupportLang::Yaml,
            ("yaml", &["yaml", "yml"], &["yaml", "yml"]),
        ),
    ];
    assert!(entries.len() == SupportLang::all_langs().len());
    let mut metadata = [entries[0].1; SupportLang::all_langs().len()];
    let mut index = 0;
    while index < entries.len() {
        metadata[entries[index].0 as usize] = entries[index].1;
        index += 1;
    }
    metadata
};
