//! The secret check: known secret formats, and generic patterns that are
//! often secrets, in the text a Practice would send.
//!
//! The rules are data, in the embedded `secret_rules.toml`, which documents
//! them: [`SecretRules::embedded`], with its version. Each names its source,
//! the provider's or the standard's documented format, and carries one
//! example it should match and one it shouldn't. A known format's match
//! blocks **Send** until the learner resolves it; a generic pattern's is only
//! a warning, since its false matches would block **Send** too often (see
//! [`Severity`]). Reading the file validates it, so [`find_secrets`] applies
//! the rules without checking them again. A finding keeps only a masked
//! excerpt, never the matched value, so logging one can't spread a secret.
//!
//! The examples are fake but match their format, so the file must not hold
//! them whole: GitHub push protection and other secret scanners would flag
//! them. Each is written as pieces that are harmless on their own, such as
//! `["AKIA", "FAKEFAKEFAKEFAKE"]`, and only exists whole in memory.

use std::cell::LazyCell;
use std::collections::HashSet;
use std::fmt;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

/// The embedded secret rules file.
const RULES_FILE: &str = include_str!("secret_rules.toml");

/// The embedded secret rules, read the first time text is checked.
static RULES: LazyLock<SecretRules> = LazyLock::new(|| {
    SecretRules::parse(RULES_FILE)
        .expect("the embedded secret rules file is valid, which a unit test checks")
});

/// The rules the secret check applies, in order, with the version of the
/// set, the regex each compiles to and what stands in for a secret.
#[derive(Debug)]
pub struct SecretRules {
    version: u32,
    rules: Vec<SecretRule>,
    /// Each rule's regex, in the rules' order.
    regexes: Vec<Regex>,
    /// What a rule's `value` group must not be all of.
    placeholders: Regex,
}

/// One secret format: a name, what its match does to **Send**, where the
/// format comes from, a regex, and an example it should match and one it
/// shouldn't.
///
/// A rule whose regex has a capture group named `value` matches only where
/// that group isn't a placeholder, such as `${DB_PASSWORD}`, `<password>`
/// or `changeme`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRule {
    name: String,
    severity: Severity,
    source: String,
    pattern: String,
    example: String,
    counterexample: String,
}

/// What a rule's match does to **Send**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// It blocks **Send** until the learner resolves it: a provider's own
    /// key format, which is rarely anything but a key.
    Blocking,
    /// It never blocks **Send**: a generic pattern, which often matches what
    /// isn't a secret.
    Warning,
}

/// The secret rules file as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    #[serde(rename = "rule")]
    rules: Vec<RuleEntry>,
    placeholders: Vec<String>,
}

/// One rule as written: its regex, example and counterexample as pieces,
/// joined in order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleEntry {
    name: String,
    severity: Severity,
    source: String,
    pattern: Vec<String>,
    example: Vec<String>,
    counterexample: Vec<String>,
}

/// Why a secret rules file cannot be used.
#[derive(Debug, thiserror::Error)]
enum SecretRulesError {
    /// It is not TOML, or not in the file's shape: an unknown or missing
    /// field, or a severity that is neither `blocking` nor `warning`.
    #[error("the secret rules file is not valid")]
    Toml(#[source] toml::de::Error),
    /// A rule has an empty name, source, pattern, example or counterexample.
    #[error("the rule {rule:?} has no {field}")]
    Empty {
        /// The rule's name, which may be the empty field.
        rule: String,
        /// The field that is empty.
        field: &'static str,
    },
    /// Two rules have the same name, so their findings would read the same.
    #[error("two rules are called {0:?}")]
    DuplicateName(String),
    /// A rule's pattern is not a regex the `regex` crate compiles.
    #[error("the rule {rule:?} has a pattern that does not compile")]
    Pattern {
        /// The rule's name.
        rule: String,
        /// Why it does not compile.
        #[source]
        source: regex::Error,
    },
    /// The placeholders do not compile.
    #[error("the placeholders do not compile")]
    Placeholders(#[source] regex::Error),
}

impl SecretRules {
    /// The rules Whetstone embeds.
    #[must_use]
    pub fn embedded() -> &'static Self {
        &RULES
    }

    /// The rules in `text`, a secret rules file. It fails if `text` is not
    /// one, or if a rule can't be applied: see [`SecretRulesError`].
    fn parse(text: &str) -> Result<Self, SecretRulesError> {
        let File {
            version,
            rules: entries,
            placeholders,
        } = toml::from_str(text).map_err(SecretRulesError::Toml)?;
        let mut names = HashSet::new();
        let mut rules = Vec::with_capacity(entries.len());
        let mut regexes = Vec::with_capacity(entries.len());
        for entry in entries {
            let rule = SecretRule {
                name: entry.name,
                severity: entry.severity,
                source: entry.source,
                pattern: entry.pattern.concat(),
                example: entry.example.concat(),
                counterexample: entry.counterexample.concat(),
            };
            if let Some(field) = rule.empty_field() {
                return Err(SecretRulesError::Empty {
                    rule: rule.name,
                    field,
                });
            }
            if !names.insert(rule.name.clone()) {
                return Err(SecretRulesError::DuplicateName(rule.name));
            }
            let regex = Regex::new(&rule.pattern).map_err(|source| SecretRulesError::Pattern {
                rule: rule.name.clone(),
                source,
            })?;
            rules.push(rule);
            regexes.push(regex);
        }
        let alternatives: Vec<String> = placeholders
            .iter()
            .map(|placeholder| format!("(?:{placeholder})"))
            .collect();
        let placeholders = Regex::new(&format!("^(?:{})$", alternatives.join("|")))
            .map_err(SecretRulesError::Placeholders)?;
        Ok(Self {
            version,
            rules,
            regexes,
            placeholders,
        })
    }

    /// The version of the rules. It changes whenever a rule is added,
    /// removed or changed.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The rules, in the order they apply: the known formats, which block
    /// **Send**, then the generic patterns, which only warn.
    #[must_use]
    pub fn rules(&self) -> &[SecretRule] {
        &self.rules
    }

    /// The possible secrets in `text`, as [`find_secrets`] finds them with
    /// these rules.
    fn find(&'static self, text: &str) -> Vec<SecretFinding> {
        let mut matches: Vec<_> = self
            .rules
            .iter()
            .zip(&self.regexes)
            .flat_map(|(rule, regex)| {
                regex.captures_iter(text).filter_map(move |captures| {
                    let value = captures.name("value");
                    if value.is_some_and(|value| self.is_placeholder(value.as_str())) {
                        return None;
                    }
                    Some((rule, captures.get(0).expect("group 0 is the whole match")))
                })
            })
            .collect();
        matches.sort_by_key(|(_, found)| found.start());
        let blocking: Vec<Range<usize>> = matches
            .iter()
            .filter(|(rule, _)| rule.severity == Severity::Blocking)
            .map(|(_, found)| found.range())
            .collect();
        matches.retain(|(rule, found)| {
            rule.severity == Severity::Blocking
                || !blocking
                    .iter()
                    .any(|range| range.start < found.end() && found.start() < range.end)
        });
        // The byte offset where each line starts, found only if a rule matches.
        let line_starts = LazyCell::new(|| {
            std::iter::once(0)
                .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                .collect::<Vec<_>>()
        });
        matches
            .into_iter()
            .map(|(rule, found)| {
                let line = line_starts.partition_point(|&start| start <= found.start());
                SecretFinding {
                    line,
                    column: found.start() - line_starts[line - 1] + 1,
                    rule,
                    excerpt: found
                        .as_str()
                        .chars()
                        .take(4)
                        .chain("********".chars())
                        .collect(),
                }
            })
            .collect()
    }

    /// Whether a rule's `value` stands in for a secret: one of the
    /// placeholders matches it whole, or it is one character repeated, such
    /// as `********`.
    fn is_placeholder(&self, value: &str) -> bool {
        let mut chars = value.chars();
        let repeated = chars
            .next()
            .is_some_and(|first| chars.all(|other| other == first));
        repeated || self.placeholders.is_match(value)
    }
}

impl SecretRule {
    /// The rule's name, which its findings show.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the rule's match does to **Send**.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Where the format comes from: the provider's or the standard's
    /// documentation.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The regex the rule matches, in `regex` crate syntax.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// A fake secret the rule should find.
    #[must_use]
    pub fn example(&self) -> &str {
        &self.example
    }

    /// A text the rule should find no secret in.
    #[must_use]
    pub fn counterexample(&self) -> &str {
        &self.counterexample
    }

    /// The first of its fields that is empty, if one is.
    fn empty_field(&self) -> Option<&'static str> {
        [
            ("name", &self.name),
            ("source", &self.source),
            ("pattern", &self.pattern),
            ("example", &self.example),
            ("counterexample", &self.counterexample),
        ]
        .into_iter()
        .find(|(_, text)| text.is_empty())
        .map(|(field, _)| field)
    }
}

/// A possible secret in a text: where it starts, the rule that matched and a
/// masked excerpt. It never holds the matched value, so its `Debug` output
/// is safe to log. Two matches are two findings even when they share a
/// line, a rule and an excerpt, since they start in different columns.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SecretFinding {
    line: usize,
    /// The 1-based byte on its line where the match starts.
    column: usize,
    rule: &'static SecretRule,
    excerpt: String,
}

impl SecretFinding {
    /// The 1-based line where the match starts.
    #[must_use]
    pub fn line(&self) -> usize {
        self.line
    }

    /// The rule that matched.
    #[must_use]
    pub fn rule(&self) -> &'static SecretRule {
        self.rule
    }

    /// The match's first four characters, then eight asterisks, however long
    /// the match is.
    #[must_use]
    pub fn excerpt(&self) -> &str {
        &self.excerpt
    }

    /// The same match, `lines` lines further down: where it is in a file
    /// when the text it was found in starts that many lines into it.
    pub(crate) fn lines_down(self, lines: usize) -> Self {
        Self {
            line: self.line + lines,
            ..self
        }
    }
}

/// Shows the rule by name alone, so a logged finding stays short.
impl fmt::Debug for SecretFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretFinding")
            .field("line", &self.line)
            .field("column", &self.column)
            .field("rule", &self.rule.name)
            .field("excerpt", &self.excerpt)
            .finish()
    }
}

/// The possible secrets in `text`, by the embedded rules, in reading order.
/// Matches that start at the same place keep the rules' order. A warning
/// that overlaps a blocking match is left out, since that match already
/// stops **Send** for the same text.
#[must_use]
pub fn find_secrets(text: &str) -> Vec<SecretFinding> {
    SecretRules::embedded().find(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_secret_rules_file_is_valid_and_every_rule_names_its_source() {
        let rules = SecretRules::parse(RULES_FILE);

        assert!(rules.is_ok(), "{rules:?}");
        for rule in SecretRules::embedded().rules() {
            assert!(rule.source().contains("https://"), "{}", rule.name());
        }
    }

    /// A secret rules file with one rule, `rule`, a TOML table's body.
    fn with_rule(rule: &str) -> String {
        format!("version = 1\nplaceholders = []\n[[rule]]\n{rule}\n")
    }

    #[test]
    fn a_rule_that_cannot_be_applied_is_refused() {
        let fields = |name: &str, severity: &str, pattern: &str, example: &str| {
            format!(
                "name = {name:?}\nseverity = {severity:?}\nsource = \"RFC\"\n\
                 pattern = [{pattern:?}]\nexample = [{example:?}]\ncounterexample = [\"b\"]"
            )
        };

        assert!(matches!(
            SecretRules::parse(&with_rule(&fields("A", "blocking", "(a", "a"))),
            Err(SecretRulesError::Pattern { rule, .. }) if rule == "A"
        ));
        assert!(matches!(
            SecretRules::parse(&with_rule(&fields("A", "blocking", "a", ""))),
            Err(SecretRulesError::Empty {
                field: "example",
                ..
            })
        ));
        assert!(matches!(
            SecretRules::parse(&with_rule(&fields("A", "fatal", "a", "a"))),
            Err(SecretRulesError::Toml(_))
        ));
        let twice = format!(
            "{}\n[[rule]]\n{}",
            with_rule(&fields("A", "blocking", "a", "a")),
            fields("A", "warning", "b", "b")
        );
        assert!(matches!(
            SecretRules::parse(&twice),
            Err(SecretRulesError::DuplicateName(name)) if name == "A"
        ));
    }

    fn rule(name: &str) -> &'static SecretRule {
        SecretRules::embedded()
            .rules()
            .iter()
            .find(|rule| rule.name() == name)
            .expect("the rule exists")
    }

    #[test]
    fn a_finding_has_its_line_rule_and_masked_excerpt() {
        let aws = rule("AWS access key ID").example();
        let text = format!("[default]\nregion = eu-west-1\naws_access_key_id = {aws}\n");

        let findings = find_secrets(&text);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line(), 3);
        assert_eq!(findings[0].rule().name(), "AWS access key ID");
        assert_eq!(findings[0].excerpt(), "AKIA********");
    }

    #[test]
    fn a_finding_never_reveals_the_matched_value() {
        let aws = rule("AWS access key ID").example();

        let findings = find_secrets(aws);

        assert_eq!(findings.len(), 1);
        assert!(!format!("{findings:?}").contains(aws));
        assert!(!findings[0].excerpt().contains(aws));
    }

    #[test]
    fn findings_are_in_reading_order() {
        let aws = rule("AWS access key ID").example();
        let github = rule("GitHub token").example();
        let text = format!("{github}\n{aws} {github}\n{aws}\n");

        let found: Vec<_> = find_secrets(&text)
            .iter()
            .map(|finding| (finding.line(), finding.rule().name()))
            .collect();

        assert_eq!(
            found,
            [
                (1, "GitHub token"),
                (2, "AWS access key ID"),
                (2, "GitHub token"),
                (3, "AWS access key ID"),
            ]
        );
    }

    #[test]
    fn the_known_secret_formats_block_send_and_the_generic_patterns_warn() {
        let rules: Vec<_> = SecretRules::embedded()
            .rules()
            .iter()
            .map(|rule| (rule.name(), rule.severity()))
            .collect();

        assert_eq!(SecretRules::embedded().version(), 2);
        assert_eq!(
            rules,
            [
                ("PEM private key", Severity::Blocking),
                ("AWS access key ID", Severity::Blocking),
                ("GitHub token", Severity::Blocking),
                ("GitLab personal access token", Severity::Blocking),
                ("Slack token", Severity::Blocking),
                ("Stripe live key", Severity::Blocking),
                ("Google API key", Severity::Blocking),
                ("OpenRouter API key", Severity::Blocking),
                ("Anthropic API key", Severity::Blocking),
                ("OpenAI API key", Severity::Blocking),
                ("Password in a URL", Severity::Warning),
                ("JSON Web Token", Severity::Warning),
                ("Literal assigned to a secret name", Severity::Warning),
                (
                    "Unquoted value assigned to a secret name",
                    Severity::Warning
                ),
                ("Start of a PEM private key", Severity::Warning),
            ]
        );
    }

    #[test]
    fn placeholders_are_never_a_warning() {
        for placeholder in [
            "${DB_PASSWORD}",
            "$DB_PASSWORD",
            "<password>",
            "{{ db_password }}",
            "%(password)s",
            "changeme",
            "CHANGE_ME",
            "your-api-key",
            "********",
            "xxxxxxxx",
            "{password}",
            "[REDACTED]",
            "REPLACE_ME",
            "sk-...",
            "settings.DB_PASSWORD",
        ] {
            let url = format!("postgres://app:{placeholder}@db.internal/app");
            let assigned = format!("db_password = \"{placeholder}\"");

            assert_eq!(rule_names(&url), [] as [&str; 0], "{url}");
            assert_eq!(rule_names(&assigned), [] as [&str; 0], "{assigned}");
        }
    }

    #[test]
    fn a_warning_inside_a_blocking_match_is_left_out() {
        let github = rule("GitHub token").example();
        let pem = rule("PEM private key")
            .example()
            .replace("FAKE", &"FAKE".repeat(8));
        let escaped = format!(r#"{{"private_key": "{}"}}"#, pem.replace('\n', r"\n"));

        assert_eq!(
            rule_names(&format!("GITHUB_TOKEN = \"{github}\"")),
            ["GitHub token"]
        );
        assert_eq!(rule_names(&escaped), ["PEM private key"]);
    }

    #[test]
    fn a_password_in_a_url_is_found_with_or_without_a_user_name() {
        let found = [
            concat!("redis://:", "hunter22@cache:6379"),
            concat!("DATABASE_URL=mysql://root:", "s3cr3t@localhost/app"),
            concat!("jdbc:postgresql://app:", "hunter2@db/app?ssl=true"),
        ];
        let kept = [
            "https://example.com:8443/path",
            "ssh://git@github.com/monadial/whetstone.git",
            "postgres://app:@db.internal/app",
            "see http://localhost:8080/a@b",
        ];

        for text in found {
            assert_eq!(rule_names(text), ["Password in a URL"], "{text}");
        }
        for text in kept {
            assert_eq!(rule_names(text), [] as [&str; 0], "{text}");
        }
    }

    #[test]
    fn a_literal_assigned_to_a_secret_name_is_found_however_it_is_assigned() {
        let found = [
            concat!("password: \"", "hunter2\""),
            concat!("\"api_key\": \"", "FAKEfake1234\""),
            concat!("const API_KEY: &str = \"", "FAKEfake1234\";"),
            concat!("token := \"", "FAKEfake1234\""),
            concat!("'client_secret' => '", "FAKEfake1234',"),
            concat!("export GITHUB_TOKEN=\"", "FAKEfake1234\""),
            concat!("AWS_SECRET_ACCESS_KEY = '", "FAKEfake1234'"),
            concat!("client(apiKey=\"", "FAKEfake1234\")"),
        ];
        let kept = [
            "password = get_password()",
            "tokenizer = \"bert-base-uncased\"",
            "password_policy = \"strict-mode\"",
            "token_type: \"bearer-token\"",
            "secret = \"short\"",
            "label = \"Enter your password\"",
            "print(\"password: \" + password + \"!\")",
            "if password == \"hunter2\":",
        ];

        for text in found {
            assert_eq!(
                rule_names(text),
                ["Literal assigned to a secret name"],
                "{text}"
            );
        }
        for text in kept {
            assert_eq!(rule_names(text), [] as [&str; 0], "{text}");
        }
    }

    #[test]
    fn an_unquoted_value_is_found_where_configuration_sets_one() {
        let found = [
            concat!("    POSTGRES_PASSWORD: ", "hunter2"),
            concat!("ENV DB_PASSWORD=", "hunter2"),
            concat!("DB_PASSWORD=", "hunter2"),
            concat!("export API_TOKEN=", "FAKEfake1234"),
            concat!("  password: ", "hunter2  # local only"),
            concat!("spring.datasource.password: ", "hunter2\r"),
        ];
        let kept = [
            "password = new_password",
            "TOKEN = os.environ.get(\"TOKEN\")",
            "SECRET_KEY = get_random_secret_key()",
            "DB_PASSWORD=${DB_PASSWORD}",
            "token: ${{ secrets.GITHUB_TOKEN }}",
            "password: !vault |",
            "secretName: db-credentials",
            "password: String,",
            "password: Optional[str]",
            "TOKEN = None",
        ];

        for text in found {
            assert_eq!(
                rule_names(text),
                ["Unquoted value assigned to a secret name"],
                "{text}"
            );
        }
        for text in kept {
            assert_eq!(rule_names(text), [] as [&str; 0], "{text}");
        }
    }

    #[test]
    fn a_format_string_that_builds_a_url_from_names_is_no_warning() {
        for text in [
            r#"f"postgresql://{user}:{password}@{host}/app""#,
            r#"format!("postgres://{user}:{password}@{host}/app")"#,
        ] {
            assert_eq!(rule_names(text), [] as [&str; 0], "{text}");
        }
    }

    #[test]
    fn a_literal_assigned_to_a_secret_name_shows_only_the_names_start() {
        let findings = find_secrets(concat!("db_password = \"", "hunter2\""));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].excerpt(), "db_p********");
    }

    #[test]
    fn the_start_of_a_pem_private_key_is_found_when_strings_split_it() {
        let body = "FAKEfakeFAKEfakeFAKEfakeFAKEfake";
        let python = format!(
            "KEY = (\"{}\\n\"\n       \"{body}\\n\")\n",
            concat!("-----BEGIN ", "RSA PRIVATE KEY-----")
        );
        let javascript = format!(
            "const key = '{}\\n' +\n  '{body}';\n",
            concat!("-----BEGIN ", "PRIVATE KEY-----")
        );
        let cut_off = format!("+{}\n+{body}\n", concat!("-----BEGIN ", "PRIVATE KEY-----"));

        for text in [python, javascript, cut_off] {
            let found: Vec<_> = find_secrets(&text)
                .iter()
                .map(|finding| (finding.line(), finding.rule().name()))
                .collect();

            assert_eq!(found, [(1, "Start of a PEM private key")], "{text}");
        }
    }

    #[test]
    fn every_rule_finds_its_example_and_not_its_counterexample() {
        for rule in SecretRules::embedded().rules() {
            let found = |text| {
                find_secrets(text)
                    .iter()
                    .map(|finding| (finding.line(), finding.rule().name()))
                    .collect::<Vec<_>>()
            };

            assert_eq!(found(rule.example()), [(1, rule.name())], "{}", rule.name());
            assert_eq!(found(rule.counterexample()), [], "{}", rule.name());
        }
    }

    #[test]
    fn a_pem_block_is_one_finding_on_its_begin_line() {
        let pem = rule("PEM private key").example();
        let aws = rule("AWS access key ID").example();
        let text = format!("# Deploy key\n\n{pem}\nkey_id = {aws}\n");

        let findings = find_secrets(&text);

        let found: Vec<_> = findings
            .iter()
            .map(|finding| (finding.line(), finding.rule().name(), finding.excerpt()))
            .collect();
        assert_eq!(
            found,
            [
                (3, "PEM private key", "----********"),
                (6, "AWS access key ID", "AKIA********"),
            ]
        );
    }

    #[test]
    fn a_pem_block_is_found_in_a_diff() {
        let pem = rule("PEM private key").example();
        let added = format!("+{}\n", pem.replace('\n', "\n+"));
        let removed = format!("-{}\n", pem.replace('\n', "\n-"));
        let diff = format!("@@ -0,0 +1,3 @@\n{added}@@ -1,3 +0,0 @@\n{removed}");

        let lines: Vec<_> = find_secrets(&diff)
            .iter()
            .map(SecretFinding::line)
            .collect();

        assert_eq!(lines, [2, 6]);
    }

    #[test]
    fn a_pem_block_escaped_inside_a_string_is_found() {
        let pem = rule("PEM private key").example();
        let json = format!(r#"{{"private_key": "{}\n"}}"#, pem.replace('\n', r"\n"));

        assert_eq!(rule_names(&json), ["PEM private key"]);
    }

    #[test]
    fn code_that_names_pem_boundaries_is_not_a_finding() {
        let code = concat!(
            "const BEGIN: &str = \"-----BEGIN ",
            "PRIVATE KEY-----\";\n",
            "const END: &str = \"-----END ",
            "PRIVATE KEY-----\";\n"
        );

        assert_eq!(rule_names(code), [] as [&str; 0]);
    }

    #[test]
    fn a_pem_block_never_runs_across_another_boundary() {
        let pem = rule("PEM private key").example();
        let unfinished = concat!("-----BEGIN ", "PRIVATE KEY-----\nFAKE\n");
        let certificate = "-----BEGIN CERTIFICATE-----\nFAKE\n-----END CERTIFICATE-----\n";
        let text = format!("{unfinished}{certificate}{pem}\n");

        let lines: Vec<_> = find_secrets(&text)
            .iter()
            .map(SecretFinding::line)
            .collect();

        assert_eq!(lines, [6]);
    }

    /// `body` repeated until it is `len` characters long.
    fn filler(body: &str, len: usize) -> String {
        body.chars().cycle().take(len).collect()
    }

    fn rule_names(text: &str) -> Vec<&'static str> {
        find_secrets(text)
            .iter()
            .map(|finding| finding.rule().name())
            .collect()
    }

    #[test]
    fn every_documented_prefix_is_found() {
        let fake = |prefix: &str, len| format!("{prefix}{}", filler("FAKEfake", len));
        let cases = [
            (fake("ASIA", 16).to_uppercase(), "AWS access key ID"),
            (fake("gho_", 36), "GitHub token"),
            (fake("ghu_", 36), "GitHub token"),
            (fake("ghs_", 36), "GitHub token"),
            (fake("ghr_", 36), "GitHub token"),
            (fake("github_pat_", 82), "GitHub token"),
            (fake("xoxp-", 10), "Slack token"),
            (fake("xapp-", 10), "Slack token"),
            (fake("xoxe.xoxp-", 10), "Slack token"),
            (fake("rk_live_", 10), "Stripe live key"),
            (
                format!(
                    "sk-proj-{0}{1}{0}",
                    filler("FAKEfake", 58),
                    "T3Blbk".to_owned() + "FJ"
                ),
                "OpenAI API key",
            ),
            (fake("sk-ant-admin01-", 32), "Anthropic API key"),
        ];

        for (text, rule_name) in cases {
            assert_eq!(rule_names(&text), [rule_name], "{rule_name}");
        }
    }

    #[test]
    fn every_pem_private_key_label_is_found() {
        for label in [
            "PRIVATE KEY",
            "ENCRYPTED PRIVATE KEY",
            "RSA PRIVATE KEY",
            "OPENSSH PRIVATE KEY",
            "PGP PRIVATE KEY BLOCK",
        ] {
            let block = format!("-----BEGIN {label}-----\nFAKE\n-----END {label}-----\n");

            assert_eq!(rule_names(&block), ["PEM private key"], "{label}");
        }
    }
}
