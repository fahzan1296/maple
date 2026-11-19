//! Language detection from file paths.
//! Reference: `l/src/lang-from-path.ts`.
//!
//! Ports `l/src/lang-from-path.ts` plus the language brand-color table from
//! `l/src/theme/theme-class.ts`. The extension table maps an extension segment
//! to `[highlight language id, LSP language id]`; highlight ids match the
//! tree-sitter / native highlighter, LSP ids match the Language Server
//! Protocol.

/// Extension segment → `[highlight language id, LSP language id]`.
pub const EXTENSION_LANG: &[(&str, &str, &str)] = &[
    // TypeScript / JavaScript
    ("ts", "typescript", "typescript"),
    ("cts", "typescript", "typescript"),
    ("mts", "typescript", "typescript"),
    ("tsx", "tsx", "typescriptreact"),
    ("js", "javascript", "javascript"),
    ("jsx", "javascript", "javascriptreact"),
    ("mjs", "javascript", "javascript"),
    ("cjs", "javascript", "javascript"),
    // Systems
    ("rs", "rust", "rust"),
    ("go", "go", "go"),
    ("c", "c", "c"),
    ("h", "c", "c"),
    ("cpp", "cpp", "cpp"),
    ("cc", "cpp", "cpp"),
    ("cxx", "cpp", "cpp"),
    ("hh", "cpp", "cpp"),
    ("hpp", "cpp", "cpp"),
    ("hxx", "cpp", "cpp"),
    ("cu", "cpp", "cpp"),
    ("cuh", "cpp", "cpp"),
    ("ino", "cpp", "cpp"),
    ("zig", "zig", "zig"),
    // Scripting
    ("py", "python", "python"),
    ("pyi", "python", "python"),
    ("rb", "ruby", "ruby"),
    ("rbw", "ruby", "ruby"),
    ("gemspec", "ruby", "ruby"),
    ("erb", "erb", "erb"),
    ("lua", "lua", "lua"),
    ("sh", "bash", "shellscript"),
    ("bash", "bash", "shellscript"),
    ("zsh", "bash", "shellscript"),
    ("ksh", "bash", "shellscript"),
    ("bats", "bash", "shellscript"),
    ("tmux", "bash", "shellscript"),
    ("cgi", "bash", "shellscript"),
    ("fcgi", "bash", "shellscript"),
    ("command", "bash", "shellscript"),
    ("tool", "bash", "shellscript"),
    ("fish", "fish", "fish"),
    ("pl", "perl", "perl"),
    ("pm", "perl", "perl"),
    ("perl", "perl", "perl"),
    ("php", "php", "php"),
    // JVM
    ("java", "java", "java"),
    ("kt", "kotlin", "kotlin"),
    ("ktm", "kotlin", "kotlin"),
    ("kts", "kotlin", "kotlin"),
    ("scala", "scala", "scala"),
    ("sc", "scala", "scala"),
    ("sbt", "scala", "scala"),
    ("groovy", "groovy", "groovy"),
    ("clj", "clojure", "clojure"),
    ("cljc", "clojure", "clojure"),
    ("cljs", "clojure", "clojure"),
    ("edn", "clojure", "clojure"),
    ("el", "emacs-lisp", "emacs-lisp"),
    // .NET
    ("cs", "csharp", "csharp"),
    ("fs", "fsharp", "fsharp"),
    ("vb", "vb", "vb"),
    // Web
    ("html", "html", "html"),
    ("htm", "html", "html"),
    ("xhtml", "html", "html"),
    ("css", "css", "css"),
    ("scss", "scss", "scss"),
    ("sass", "sass", "sass"),
    ("less", "less", "less"),
    ("vue", "vue", "vue"),
    ("svelte", "svelte", "svelte"),
    ("astro", "astro", "astro"),
    // Data
    ("json", "json", "json"),
    ("jsonc", "jsonc", "jsonc"),
    ("yaml", "yaml", "yaml"),
    ("yml", "yaml", "yaml"),
    ("toml", "toml", "toml"),
    ("xml", "xml", "xml"),
    ("xsl", "xml", "xml"),
    ("xslt", "xml", "xml"),
    ("svg", "xml", "xml"),
    ("plist", "xml", "xml"),
    ("ini", "ini", "ini"),
    // Docs
    ("md", "markdown", "markdown"),
    ("markdown", "markdown", "markdown"),
    ("mdx", "markdown", "markdown"),
    ("mdc", "markdown", "markdown"),
    ("mkd", "markdown", "markdown"),
    ("mdown", "markdown", "markdown"),
    ("rst", "restructuredtext", "restructuredtext"),
    ("adoc", "asciidoc", "asciidoc"),
    ("tex", "latex", "latex"),
    // Other languages
    ("sql", "sql", "sql"),
    ("graphql", "graphql", "graphql"),
    ("gql", "graphql", "graphql"),
    ("proto", "protobuf", "protobuf"),
    ("dockerfile", "dockerfile", "dockerfile"),
    ("containerfile", "dockerfile", "dockerfile"),
    ("tf", "hcl", "terraform"),
    ("hcl", "hcl", "hcl"),
    ("tfvars", "hcl", "hcl"),
    ("nix", "nix", "nix"),
    ("ex", "elixir", "elixir"),
    ("exs", "elixir", "elixir"),
    ("erl", "erlang", "erlang"),
    ("hrl", "erlang", "erlang"),
    ("hs", "haskell", "haskell"),
    ("ml", "ocaml", "ocaml"),
    ("mli", "ocaml", "ocaml"),
    ("swift", "swift", "swift"),
    ("r", "r", "r"),
    ("jl", "julia", "julia"),
    ("dart", "dart", "dart"),
    ("elm", "elm", "elm"),
    ("v", "verilog", "v"),
    ("nim", "nim", "nim"),
    ("cr", "crystal", "crystal"),
    ("d", "d", "d"),
    ("pas", "pascal", "pascal"),
    ("pp", "pascal", "pascal"),
    ("lisp", "lisp", "lisp"),
    ("lsp", "lisp", "lisp"),
    ("rkt", "racket", "racket"),
    ("scm", "scheme", "scheme"),
    ("ps1", "powershell", "powershell"),
    ("psm1", "powershell", "powershell"),
    ("bat", "bat", "bat"),
    ("cmd", "bat", "bat"),
    ("tla", "tlaplus", "tlaplus"),
    ("tlaplus", "tlaplus", "tlaplus"),
    ("m", "objc", "plaintext"),
    ("mm", "objc", "plaintext"),
    ("sol", "solidity", "plaintext"),
    ("odin", "odin", "plaintext"),
    ("star", "starlark", "plaintext"),
    ("bzl", "starlark", "plaintext"),
    ("sv", "verilog", "plaintext"),
    ("svh", "verilog", "plaintext"),
    ("vh", "verilog", "plaintext"),
    ("vim", "vim", "plaintext"),
    ("ipynb", "ipynb", "plaintext"),
    ("hbs", "handlebars", "plaintext"),
    ("hsb", "handlebars", "plaintext"),
    ("handlebars", "handlebars", "plaintext"),
    ("diff", "diff", "plaintext"),
    ("patch", "diff", "plaintext"),
    ("makefile", "make", "plaintext"),
    ("mk", "make", "plaintext"),
    ("mak", "make", "plaintext"),
    ("cmake", "cmake", "cmake"),
    ("justfile", "just", "plaintext"),
    ("txt", "text", "plaintext"),
    ("text", "text", "plaintext"),
    ("log", "log", "plaintext"),
    ("csv", "csv", "plaintext"),
    ("tsv", "tsv", "plaintext"),
    ("cfg", "conf", "plaintext"),
    ("conf", "conf", "plaintext"),
    ("config", "conf", "plaintext"),
    ("properties", "conf", "plaintext"),
    ("env", "env", "plaintext"),
    ("gitignore", "conf", "plaintext"),
    ("gitattributes", "conf", "plaintext"),
    ("gitmodules", "conf", "plaintext"),
    ("editorconfig", "conf", "plaintext"),
    ("npmrc", "conf", "plaintext"),
    ("prettierrc", "conf", "plaintext"),
    ("eslintrc", "conf", "plaintext"),
    ("prettierignore", "conf", "plaintext"),
    ("eslintignore", "conf", "plaintext"),
];

/// Brand colors for language icons, keyed by the resolved `lang.*` symbol key.
/// Languages without an entry fall back to the muted theme color.
pub const LANG_BRAND_COLORS: &[(&str, &str)] = &[
    ("lang.javascript", "#f7df1e"),
    ("lang.python", "#3776ab"),
    ("lang.ruby", "#cc342d"),
    ("lang.julia", "#9558b2"),
];

/// Look up an extension segment (without a leading dot) in [`EXTENSION_LANG`].
fn lookup_extension(extension: &str) -> Option<(&'static str, &'static str)> {
    EXTENSION_LANG
        .iter()
        .find(|(ext, _, _)| *ext == extension)
        .map(|(_, highlight, lsp)| (*highlight, *lsp))
}

/// Final path segment. Splits on both `/` and `\` so Windows paths behave the
/// same on every host (Node's `path.basename` follows the host separator).
fn path_basename(file_path: &str) -> &str {
    file_path
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(file_path)
}

/// Final segment after the last `.` in the full path (prior theme behavior).
fn theme_extension_key(file_path: &str) -> String {
    match file_path.rfind('.') {
        Some(ext_beg) => file_path[ext_beg + 1..].to_lowercase(),
        None => file_path.to_lowercase(),
    }
}

/// Lowercased extension of the final segment, without the leading dot.
fn lsp_extension_key(file_path: &str) -> String {
    let base = path_basename(file_path);
    match base.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < base.len() => base[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// Language id for syntax highlighting and UI (icons, read tool), or `None` if
/// unknown.
pub fn get_language_from_path(file_path: &str) -> Option<&'static str> {
    let base_name = path_basename(file_path).to_lowercase();
    if base_name.starts_with(".env.") {
        return Some("env");
    }
    if base_name == "dockerfile" || base_name.starts_with("dockerfile.") || base_name == "containerfile" {
        return Some("dockerfile");
    }
    if base_name == ".emacs" {
        return Some("emacs-lisp");
    }
    if base_name == "justfile" {
        return Some("just");
    }
    if base_name == "cmakelists.txt" {
        return Some("cmake");
    }

    lookup_extension(&theme_extension_key(file_path))
        .map(|(highlight, _)| highlight)
}

/// Whether `file_path` is a Markdown document.
pub fn is_markdown_path(file_path: &str) -> bool {
    get_language_from_path(file_path) == Some("markdown")
}

/// LSP language identifier; falls back to `plaintext`.
pub fn detect_language_id(file_path: &str) -> String {
    let base_name = path_basename(file_path).to_lowercase();
    if base_name == "dockerfile" || base_name.starts_with("dockerfile.") || base_name == "containerfile" {
        return "dockerfile".to_string();
    }
    if base_name == ".emacs" {
        return "emacs-lisp".to_string();
    }
    if base_name == "makefile" || base_name == "gnumakefile" {
        return "makefile".to_string();
    }
    if base_name == "justfile" {
        return "just".to_string();
    }

    let lsp_ext = lsp_extension_key(file_path);
    if base_name == "cmakelists.txt" || lsp_ext == "cmake" {
        return "cmake".to_string();
    }

    lookup_extension(&lsp_ext)
        .map(|(_, lsp)| lsp.to_string())
        .unwrap_or_else(|| "plaintext".to_string())
}

/// Resolve the brand color for a `lang.*` symbol key.
pub fn brand_color(symbol_key: &str) -> Option<&'static str> {
    LANG_BRAND_COLORS
        .iter()
        .find(|(key, _)| *key == symbol_key)
        .map(|(_, color)| *color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_common_extensions() {
        assert_eq!(get_language_from_path("src/main.rs"), Some("rust"));
        assert_eq!(get_language_from_path("src/index.ts"), Some("typescript"));
        assert_eq!(get_language_from_path("script.py"), Some("python"));
        assert_eq!(get_language_from_path("styles.css"), Some("css"));
        assert_eq!(get_language_from_path("data.json"), Some("json"));
    }

    #[test]
    fn detects_dockerfile_makefile_and_justfile() {
        assert_eq!(get_language_from_path("Dockerfile"), Some("dockerfile"));
        assert_eq!(get_language_from_path("dockerfile.prod"), Some("dockerfile"));
        assert_eq!(get_language_from_path("Containerfile"), Some("dockerfile"));
        assert_eq!(get_language_from_path("Makefile"), Some("make"));
        assert_eq!(get_language_from_path("justfile"), Some("just"));
        assert_eq!(detect_language_id("Makefile"), "makefile");
        assert_eq!(detect_language_id("GNUmakefile"), "makefile");
        assert_eq!(detect_language_id("justfile"), "just");
        assert_eq!(detect_language_id("Dockerfile"), "dockerfile");
        assert_eq!(detect_language_id("dockerfile.prod"), "dockerfile");
    }

    #[test]
    fn detects_lsp_ids() {
        assert_eq!(detect_language_id("a.tsx"), "typescriptreact");
        assert_eq!(detect_language_id("a.jsx"), "javascriptreact");
        assert_eq!(detect_language_id("a.sh"), "shellscript");
        assert_eq!(detect_language_id("a.tf"), "terraform");
        assert_eq!(detect_language_id("a.md"), "markdown");
    }

    #[test]
    fn markdown_paths() {
        assert!(is_markdown_path("README.md"));
        assert!(is_markdown_path("docs/guide.mdx"));
        assert!(!is_markdown_path("src/main.rs"));
    }

    #[test]
    fn basename_special_cases() {
        assert_eq!(get_language_from_path(".env"), Some("env"));
        assert_eq!(get_language_from_path(".env.local"), Some("env"));
        assert_eq!(get_language_from_path(".emacs"), Some("emacs-lisp"));
        assert_eq!(get_language_from_path("CMakeLists.txt"), Some("cmake"));
        assert_eq!(detect_language_id("CMakeLists.txt"), "cmake");
        assert_eq!(detect_language_id("build.cmake"), "cmake");
        // Node's `path.extname` ignores a leading dot, so a dotfile named like
        // an extension is not treated as that language.
        assert_eq!(detect_language_id(".ts"), "plaintext");
        assert_eq!(detect_language_id("..ts"), "typescript");
    }

    #[test]
    fn unknown_extensions_fall_back_to_plaintext() {
        assert_eq!(get_language_from_path("notes.unknown"), None);
        assert_eq!(detect_language_id("notes.unknown"), "plaintext");
        assert_eq!(detect_language_id("no_extension"), "plaintext");
    }

    #[test]
    fn windows_paths_use_the_final_segment() {
        assert_eq!(get_language_from_path("C:\\repo\\main.rs"), Some("rust"));
        assert_eq!(detect_language_id("C:\\repo\\Dockerfile"), "dockerfile");
    }

    #[test]
    fn brand_colors_resolve_known_keys() {
        assert_eq!(brand_color("lang.javascript"), Some("#f7df1e"));
        assert_eq!(brand_color("lang.python"), Some("#3776ab"));
        assert_eq!(brand_color("lang.ruby"), Some("#cc342d"));
        assert_eq!(brand_color("lang.julia"), Some("#9558b2"));
        assert_eq!(brand_color("lang.rust"), None);
        assert_eq!(LANG_BRAND_COLORS.len(), 4);
    }
}
