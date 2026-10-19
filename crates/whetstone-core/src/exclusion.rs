//! Exclusions: the touched files a Practice leaves out of its evidence.

use std::fmt;
use std::iter::Peekable;
use std::str::Chars;

use regex::Regex;

/// One gitignore-style `exclude` pattern from a repository's config, matched
/// against repository-relative paths.
#[derive(Debug, Clone)]
pub struct ExcludePattern {
    text: String,
    regex: Regex,
}

impl ExcludePattern {
    /// Parses `text` as one gitignore-style pattern.
    pub fn parse(text: &str) -> Result<Self, ExcludePatternError> {
        let invalid = |problem| ExcludePatternError {
            pattern: text.to_owned(),
            problem,
        };
        if text.starts_with('!') {
            return Err(invalid(ExcludePatternProblem::Negation));
        }
        let (rest, directory_only) = match text.strip_suffix('/') {
            Some(rest) => (rest, true),
            None => (text, false),
        };
        // A slash at the start or in the middle anchors the pattern to the
        // repository root. Otherwise it matches at any depth.
        let (rest, anchored) = match rest.strip_prefix('/') {
            Some(rest) => (rest, true),
            None => (rest, rest.contains('/')),
        };
        if rest.is_empty() {
            return Err(invalid(ExcludePatternProblem::Empty));
        }
        let body = translate(&tokenize(rest).map_err(invalid)?);
        let depth = if anchored { "" } else { "(?:.*/)?" };
        // A pattern that matches a directory leaves out everything in it, so
        // it matches the whole path or any of its leading directories.
        let inside = if directory_only { "/.*" } else { "(?:/.*)?" };
        // The translation is always valid syntax, so only the regex's size
        // limit can refuse it.
        let regex = Regex::new(&format!("(?s)^{depth}{body}{inside}$"))
            .map_err(|_| invalid(ExcludePatternProblem::TooLong))?;
        Ok(Self {
            text: text.to_owned(),
            regex,
        })
    }

    /// The pattern exactly as written, which the outbound review shows as
    /// the reason a file is left out.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the pattern leaves out the file at `path`.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        self.regex.is_match(path)
    }
}

/// Two patterns are equal when they are written the same way.
impl PartialEq for ExcludePattern {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for ExcludePattern {}

/// A repository's `exclude` patterns, in the order its config lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludePatterns(Vec<ExcludePattern>);

impl ExcludePatterns {
    /// Parses each of `texts` as a pattern, failing at the first invalid one.
    pub fn parse<I>(texts: I) -> Result<Self, ExcludePatternError>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        texts
            .into_iter()
            .map(|text| ExcludePattern::parse(text.as_ref()))
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// The first pattern that leaves out the file at `path`, or `None` if
    /// none does.
    #[must_use]
    pub fn first_match(&self, path: &str) -> Option<&ExcludePattern> {
        self.0.iter().find(|pattern| pattern.matches(path))
    }
}

/// A file left out by default, until the learner brings it back: a
/// lockfile or a common secret path. Each matches by file name, at any depth
/// and ignoring ASCII case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultExclusion {
    kind: DefaultExclusionKind,
    name_pattern: &'static str,
}

/// What kind of file a default exclusion leaves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultExclusionKind {
    /// A package manager's lockfile: bulky and of little use to a question.
    Lockfile,
    /// A path where secrets commonly live.
    SecretPath,
}

/// Lockfiles, by file name.
const LOCKFILES: [&str; 20] = [
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Gemfile.lock",
    "poetry.lock",
    "composer.lock",
    "go.sum",
    "Pipfile.lock",
    "bun.lockb",
    "bun.lock",
    "uv.lock",
    "flake.lock",
    "mix.lock",
    "pubspec.lock",
    "Podfile.lock",
    "packages.lock.json",
    "npm-shrinkwrap.json",
    "deno.lock",
    "Package.resolved",
];

/// Common secret paths, by file name. A `*` stands for any run of
/// characters, and only ever leads or trails.
const SECRET_PATHS: [&str; 16] = [
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "id_rsa*",
    "id_ed25519*",
    "*.p12",
    "*.pfx",
    "*.keystore",
    "*.jks",
    "credentials*",
    "secrets.*",
    ".npmrc",
    ".pypirc",
    ".netrc",
    "*.kdbx",
];

impl DefaultExclusion {
    /// The default exclusion that leaves out the file at `path`, relative to
    /// the repository's root, or `None` if none does.
    #[must_use]
    pub fn of(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let lists = [
            (DefaultExclusionKind::Lockfile, LOCKFILES.as_slice()),
            (DefaultExclusionKind::SecretPath, SECRET_PATHS.as_slice()),
        ];
        lists.into_iter().find_map(|(kind, name_patterns)| {
            name_patterns
                .iter()
                .find(|name_pattern| name_matches(name_pattern, name))
                .map(|&name_pattern| Self { kind, name_pattern })
        })
    }

    /// What kind of file it leaves out.
    #[must_use]
    pub fn kind(self) -> DefaultExclusionKind {
        self.kind
    }

    /// The file name it matched, such as `Cargo.lock` or `*.pem`.
    #[must_use]
    pub fn name_pattern(self) -> &'static str {
        self.name_pattern
    }
}

impl DefaultExclusionKind {
    /// Why a file of this kind is left out, as the outbound review shows it.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::Lockfile => "lockfile",
            Self::SecretPath => "common secret path",
        }
    }
}

/// Whether the file `name` matches `name_pattern`, ignoring ASCII case.
fn name_matches(name_pattern: &str, name: &str) -> bool {
    let name = name.as_bytes();
    if let Some(suffix) = name_pattern.strip_prefix('*') {
        name.len() >= suffix.len()
            && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
    } else if let Some(prefix) = name_pattern.strip_suffix('*') {
        name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    } else {
        name.eq_ignore_ascii_case(name_pattern.as_bytes())
    }
}

/// One piece of a pattern, between its parsing and its translation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// A `/` between two directory levels.
    Separator,
    /// `*`, any run of characters within one directory level.
    Star,
    /// `**` as a whole level: any number of directory levels.
    Globstar,
    /// A regex fragment matching exactly one character other than `/`.
    OneChar(String),
}

/// Splits a pattern, without its anchoring or trailing slash, into tokens.
fn tokenize(pattern: &str) -> Result<Vec<Token>, ExcludePatternProblem> {
    let mut tokens = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        let token = match c {
            '/' => Token::Separator,
            '*' => {
                let mut run = 1;
                while chars.next_if_eq(&'*').is_some() {
                    run += 1;
                }
                // `**` is special only as a whole directory level. Elsewhere
                // it is a plain `*`, as in Git.
                let starts_level = matches!(tokens.last(), None | Some(Token::Separator));
                let ends_level = matches!(chars.peek(), None | Some('/'));
                if run > 1 && starts_level && ends_level {
                    Token::Globstar
                } else {
                    Token::Star
                }
            }
            '?' => Token::OneChar("[^/]".to_owned()),
            '[' => Token::OneChar(class(&mut chars)?),
            '\\' => {
                let escaped = chars
                    .next()
                    .ok_or(ExcludePatternProblem::TrailingBackslash)?;
                Token::OneChar(literal(escaped))
            }
            c => Token::OneChar(literal(c)),
        };
        tokens.push(token);
    }
    Ok(tokens)
}

/// The POSIX classes a bracket expression can name, as Git's do.
const POSIX_CLASSES: [&str; 12] = [
    "alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space",
    "upper", "xdigit",
];

/// Reads a bracket expression after its `[` and translates it into a regex
/// class. Like Git's, it never matches `/`.
fn class(chars: &mut Peekable<Chars<'_>>) -> Result<String, ExcludePatternProblem> {
    let negated = chars.next_if(|&c| c == '!' || c == '^').is_some();
    let mut items = String::new();
    let mut first = true;
    loop {
        let c = chars.next().ok_or(ExcludePatternProblem::UnclosedClass)?;
        match c {
            // A `]` straight after the `[` (or its negation) is literal.
            ']' if !first => break,
            '[' if chars.peek() == Some(&':') => {
                // As in Git, `[:` starts a class name only when the next `]`
                // follows a `:`. Otherwise the `[` is literal.
                let mut after = chars.clone();
                after.next();
                let mut inner = String::new();
                while let Some(c) = after.next_if(|&c| c != ']') {
                    inner.push(c);
                }
                match inner.strip_suffix(':') {
                    Some(name) if after.next() == Some(']') => {
                        if !POSIX_CLASSES.contains(&name) {
                            return Err(ExcludePatternProblem::UnknownClass);
                        }
                        *chars = after;
                        items.push_str("[:");
                        items.push_str(name);
                        items.push_str(":]");
                    }
                    _ => items.push_str(&literal('[')),
                }
            }
            c => {
                let start = escaped(c, chars)?;
                items.push_str(&literal(start));
                // A `-` before the closing `]` is literal.
                if chars.peek() == Some(&'-') && chars.clone().nth(1).is_some_and(|c| c != ']') {
                    chars.next();
                    let end = chars.next().ok_or(ExcludePatternProblem::UnclosedClass)?;
                    let end = escaped(end, chars)?;
                    if end < start {
                        return Err(ExcludePatternProblem::ReversedRange);
                    }
                    items.push('-');
                    items.push_str(&literal(end));
                }
            }
        }
        first = false;
    }
    Ok(if negated {
        format!("[^/{items}]")
    } else {
        format!("[{items}&&[^/]]")
    })
}

/// The character `c` stands for inside a bracket expression, reading the one
/// after it when `c` is a backslash.
fn escaped(c: char, chars: &mut Peekable<Chars<'_>>) -> Result<char, ExcludePatternProblem> {
    if c == '\\' {
        chars.next().ok_or(ExcludePatternProblem::UnclosedClass)
    } else {
        Ok(c)
    }
}

/// A regex fragment matching exactly `c`.
fn literal(c: char) -> String {
    regex::escape(c.encode_utf8(&mut [0; 4]))
}

/// Translates tokens into the body of a regex over `/`-separated paths.
fn translate(tokens: &[Token]) -> String {
    let mut body = String::new();
    let mut tokens = tokens.iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            Token::Separator => body.push('/'),
            Token::Star => body.push_str("[^/]*"),
            Token::Globstar => {
                if tokens.next_if_eq(&&Token::Separator).is_some() {
                    // `**/` matches zero or more whole directory levels.
                    body.push_str("(?:.*/)?");
                } else {
                    // A trailing `**` matches everything inside.
                    body.push_str(".*");
                }
            }
            Token::OneChar(fragment) => body.push_str(fragment),
        }
    }
    body
}

/// An `exclude` pattern that can't be parsed, with its problem.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid exclude pattern `{pattern}`: {problem}")]
pub struct ExcludePatternError {
    pattern: String,
    problem: ExcludePatternProblem,
}

impl ExcludePatternError {
    /// The pattern exactly as written.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Why it is invalid.
    #[must_use]
    pub fn problem(&self) -> ExcludePatternProblem {
        self.problem
    }
}

/// Why an `exclude` pattern is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcludePatternProblem {
    /// It is empty, or only a slash.
    Empty,
    /// It starts with `!`, which in a gitignore file brings files back.
    Negation,
    /// A `[` has no closing `]`.
    UnclosedClass,
    /// A character range such as `z-a` runs backwards.
    ReversedRange,
    /// A `[:name:]` class names none of Git's classes.
    UnknownClass,
    /// It ends with a `\` that escapes nothing.
    TrailingBackslash,
    /// It is too long to match.
    TooLong,
}

impl fmt::Display for ExcludePatternProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "it is empty",
            Self::Negation => {
                "`!` can't bring files back, because exclude patterns only leave files out"
            }
            Self::UnclosedClass => "a `[` has no closing `]`",
            Self::ReversedRange => "a character range runs backwards",
            Self::UnknownClass => "a `[:name:]` class names no known class",
            Self::TrailingBackslash => "it ends with a `\\` that escapes nothing",
            Self::TooLong => "it is too long",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(text: &str) -> ExcludePattern {
        ExcludePattern::parse(text).expect("the pattern is valid")
    }

    #[test]
    fn an_unanchored_glob_matches_a_file_name_at_any_depth() {
        let logs = pattern("*.log");

        assert!(logs.matches("debug.log"));
        assert!(logs.matches("var/log/debug.log"));
        assert!(!logs.matches("debug.log.txt"));
        assert!(!logs.matches("debug.txt"));
    }

    #[test]
    fn a_pattern_that_matches_a_directory_leaves_out_everything_in_it() {
        let vendor = pattern("vendor");

        assert!(vendor.matches("vendor"));
        assert!(vendor.matches("vendor/lib/a.rs"));
        assert!(vendor.matches("web/vendor/b.js"));
        assert!(!vendor.matches("vendored/a.rs"));
        assert!(!vendor.matches("src/vendor.rs"));
    }

    #[test]
    fn a_directory_pattern_matches_only_directories() {
        let build = pattern("build/");

        assert!(build.matches("build/out.o"));
        assert!(build.matches("web/build/app.js"));
        assert!(!build.matches("build"));
        assert!(!build.matches("tools/build"));
    }

    #[test]
    fn a_leading_slash_anchors_a_pattern_to_the_repository_root() {
        let notes = pattern("/notes.md");

        assert!(notes.matches("notes.md"));
        assert!(!notes.matches("docs/notes.md"));
    }

    #[test]
    fn a_slash_in_the_middle_anchors_a_pattern_to_the_repository_root() {
        let fixtures = pattern("tests/fixtures");
        let generated = pattern("src/*.generated.rs");

        assert!(fixtures.matches("tests/fixtures/big.json"));
        assert!(!fixtures.matches("crates/core/tests/fixtures/big.json"));
        assert!(generated.matches("src/api.generated.rs"));
        assert!(!generated.matches("src/v1/api.generated.rs"));
        assert!(!generated.matches("lib/src/api.generated.rs"));
    }

    #[test]
    fn an_anchored_directory_pattern_matches_only_that_directory() {
        let dist = pattern("/dist/");

        assert!(dist.matches("dist/app.js"));
        assert!(!dist.matches("dist"));
        assert!(!dist.matches("web/dist/app.js"));
    }

    #[test]
    fn a_leading_double_star_matches_in_every_directory() {
        let snapshots = pattern("**/snapshots/*.snap");

        assert!(snapshots.matches("snapshots/home.snap"));
        assert!(snapshots.matches("crates/app/tests/snapshots/home.snap"));
        assert!(!snapshots.matches("snapshots/old/home.snap"));
    }

    #[test]
    fn a_trailing_double_star_matches_everything_inside() {
        let assets = pattern("assets/**");

        assert!(assets.matches("assets/logo.png"));
        assert!(assets.matches("assets/brand/flat/logo.svg"));
        assert!(!assets.matches("assets"));
        assert!(!assets.matches("web/assets/logo.png"));
    }

    #[test]
    fn a_double_star_between_slashes_matches_zero_or_more_directories() {
        let data = pattern("docs/**/data.csv");

        assert!(data.matches("docs/data.csv"));
        assert!(data.matches("docs/a/data.csv"));
        assert!(data.matches("docs/a/b/data.csv"));
        assert!(!data.matches("docs/a/b/my-data.csv"));
    }

    #[test]
    fn a_double_star_inside_a_name_is_a_single_star() {
        let logs = pattern("logs/**.txt");

        assert!(logs.matches("logs/today.txt"));
        assert!(!logs.matches("logs/2026/today.txt"));
    }

    #[test]
    fn a_question_mark_matches_one_character_but_not_a_slash() {
        let drafts = pattern("draft-?.md");
        let nested = pattern("a?b");

        assert!(drafts.matches("draft-1.md"));
        assert!(!drafts.matches("draft-12.md"));
        assert!(!drafts.matches("draft-.md"));
        assert!(!nested.matches("a/b"));
    }

    #[test]
    fn a_character_class_matches_one_listed_character() {
        let reports = pattern("report-[0-9a-c].pdf");

        assert!(reports.matches("report-7.pdf"));
        assert!(reports.matches("report-b.pdf"));
        assert!(!reports.matches("report-d.pdf"));
        assert!(!reports.matches("report-77.pdf"));
    }

    #[test]
    fn a_negated_character_class_matches_one_unlisted_character() {
        let bang = pattern("v[!0-9].txt");
        let caret = pattern("v[^0-9].txt");

        for negated in [bang, caret] {
            assert!(negated.matches("vx.txt"));
            assert!(!negated.matches("v1.txt"));
            assert!(!negated.matches("v/.txt"));
        }
        assert!(!pattern("a[/]b").matches("a/b"));
    }

    #[test]
    fn a_character_class_reads_special_characters_literally() {
        let bracket = pattern("[]]");
        let symbols = pattern("[*?-].md");
        let posix = pattern("[[:digit:]].md");

        assert!(bracket.matches("]"));
        assert!(symbols.matches("*.md"));
        assert!(symbols.matches("-.md"));
        assert!(!symbols.matches("a.md"));
        assert!(posix.matches("7.md"));
        assert!(!posix.matches("d.md"));
    }

    #[test]
    fn an_open_bracket_and_colon_without_a_class_name_is_literal() {
        let colon = pattern("[[:]");
        let mixed = pattern("[a[:b]");

        assert!(colon.matches("["));
        assert!(colon.matches(":"));
        for name in ["a", "[", ":", "b"] {
            assert!(mixed.matches(name), "{name}");
        }
        assert!(!mixed.matches("c"));
    }

    #[test]
    fn a_backslash_makes_the_next_character_literal() {
        let star = pattern(r"\*.md");
        let bang = pattern(r"\!important.txt");

        assert!(star.matches("*.md"));
        assert!(!star.matches("readme.md"));
        assert!(bang.matches("!important.txt"));
    }

    #[test]
    fn a_pattern_list_reports_the_first_pattern_that_matches_as_written() {
        let patterns = ExcludePatterns::parse(["/fixtures/", "*.json", "fixtures/*.json"])
            .expect("the patterns are valid");

        let reason = |path| patterns.first_match(path).map(ExcludePattern::as_str);
        assert_eq!(reason("fixtures/big.json"), Some("/fixtures/"));
        assert_eq!(reason("web/package.json"), Some("*.json"));
        assert_eq!(reason("src/main.rs"), None);
    }

    #[test]
    fn a_pattern_list_is_rejected_at_its_first_invalid_pattern() {
        let error = ExcludePatterns::parse(["fixtures/", "!fixtures/keep.json", "["])
            .expect_err("a negation is invalid");

        assert_eq!(error.pattern(), "!fixtures/keep.json");
        assert_eq!(error.problem(), ExcludePatternProblem::Negation);
    }

    #[test]
    fn an_empty_pattern_list_leaves_nothing_out() {
        let patterns = ExcludePatterns::default();

        assert_eq!(patterns.first_match("anything.txt"), None);
    }

    fn default_exclusion(path: &str) -> Option<(DefaultExclusionKind, &'static str)> {
        DefaultExclusion::of(path).map(|exclusion| (exclusion.kind(), exclusion.name_pattern()))
    }

    #[test]
    fn lockfiles_are_left_out_by_default_at_any_depth() {
        let lockfiles = [
            "Cargo.lock",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "Gemfile.lock",
            "poetry.lock",
            "composer.lock",
            "go.sum",
            "Pipfile.lock",
            "bun.lockb",
            "bun.lock",
            "uv.lock",
            "flake.lock",
            "mix.lock",
            "pubspec.lock",
            "Podfile.lock",
            "packages.lock.json",
            "npm-shrinkwrap.json",
            "deno.lock",
            "Package.resolved",
        ];

        for name in lockfiles {
            let nested = format!("apps/web/{name}");
            assert_eq!(
                default_exclusion(name),
                Some((DefaultExclusionKind::Lockfile, name))
            );
            assert_eq!(
                default_exclusion(&nested),
                Some((DefaultExclusionKind::Lockfile, name))
            );
        }
    }

    #[test]
    fn common_secret_paths_are_left_out_by_default_at_any_depth() {
        let secret_paths = [
            (".env", ".env"),
            ("app/.env", ".env"),
            (".env.example", ".env.*"),
            ("deploy/.env.production", ".env.*"),
            ("certs/server.pem", "*.pem"),
            ("tls.key", "*.key"),
            ("home/.ssh/id_rsa", "id_rsa*"),
            ("id_rsa.pub", "id_rsa*"),
            ("id_ed25519", "id_ed25519*"),
            ("store.p12", "*.p12"),
            ("store.pfx", "*.pfx"),
            ("android/release.keystore", "*.keystore"),
            ("release.jks", "*.jks"),
            ("credentials", "credentials*"),
            ("aws/credentials.json", "credentials*"),
            ("secrets.yaml", "secrets.*"),
            (".npmrc", ".npmrc"),
            (".pypirc", ".pypirc"),
            (".netrc", ".netrc"),
            ("vault.kdbx", "*.kdbx"),
        ];

        for (path, name_pattern) in secret_paths {
            assert_eq!(
                default_exclusion(path),
                Some((DefaultExclusionKind::SecretPath, name_pattern)),
                "{path}"
            );
        }
    }

    #[test]
    fn default_exclusions_ignore_the_case_of_a_file_name() {
        assert_eq!(
            default_exclusion("certs/SERVER.PEM"),
            Some((DefaultExclusionKind::SecretPath, "*.pem"))
        );
        assert_eq!(
            default_exclusion("ID_RSA"),
            Some((DefaultExclusionKind::SecretPath, "id_rsa*"))
        );
    }

    #[test]
    fn files_that_only_resemble_a_default_exclusion_are_kept() {
        let kept = [
            "src/env.rs",
            ".envrc",
            "environment.ts",
            "docs/pem.md",
            "src/key.rs",
            "keys/README.md",
            "credentials/README.md",
            "my-credentials.json",
            "secrets",
            "Cargo.toml",
            "Cargo.lock.md",
            "go.mod",
            "lock/yarn.md",
        ];

        for path in kept {
            assert_eq!(default_exclusion(path), None, "{path}");
        }
    }

    #[test]
    fn each_default_exclusion_gives_the_reason_the_review_shows() {
        let reason = |path| DefaultExclusion::of(path).map(|exclusion| exclusion.kind().reason());

        assert_eq!(reason("Cargo.lock"), Some("lockfile"));
        assert_eq!(reason(".env.example"), Some("common secret path"));
    }

    fn problem(text: &str) -> ExcludePatternProblem {
        let error = ExcludePattern::parse(text).expect_err("the pattern is invalid");
        assert_eq!(error.pattern(), text);
        error.problem()
    }

    #[test]
    fn a_negation_is_rejected_because_config_can_only_add_exclusions() {
        let error = ExcludePattern::parse("!.env.example").expect_err("negation is invalid");

        assert_eq!(error.problem(), ExcludePatternProblem::Negation);
        assert_eq!(
            error.to_string(),
            "invalid exclude pattern `!.env.example`: `!` can't bring files back, \
             because exclude patterns only leave files out"
        );
    }

    #[test]
    fn a_malformed_pattern_is_rejected_with_its_problem() {
        assert_eq!(problem(""), ExcludePatternProblem::Empty);
        assert_eq!(problem("/"), ExcludePatternProblem::Empty);
        assert_eq!(
            problem("report-[0-9.pdf"),
            ExcludePatternProblem::UnclosedClass
        );
        assert_eq!(problem("[!]"), ExcludePatternProblem::UnclosedClass);
        assert_eq!(problem("[z-a].md"), ExcludePatternProblem::ReversedRange);
        assert_eq!(problem("[[:digits:]]"), ExcludePatternProblem::UnknownClass);
        assert_eq!(problem(r"notes\"), ExcludePatternProblem::TrailingBackslash);
    }
}
