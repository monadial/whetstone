//! The secret check: known secret formats in the text a Practice would send.
//!
//! The rules are data: [`SECRET_RULES`], versioned by
//! [`SECRET_RULES_VERSION`]. Each names its provider's documented format and
//! carries one example it should match and one it shouldn't. A finding keeps
//! only a masked excerpt, never the matched value, so logging one can't spread
//! a secret.
//!
//! The examples are fake but match their format, so the source must not hold
//! them whole: GitHub push protection and other secret scanners would flag
//! them. Each is built with `concat!` from pieces that are harmless on their
//! own, such as `concat!("AKIA", "FAKEFAKEFAKEFAKE")`, and only exists whole
//! in the compiled binary.
//!
//! Patterns use `(?-u:\b)`, an ASCII word boundary: every format is ASCII, and
//! the crate leaves out `regex`'s Unicode tables.

use std::cell::LazyCell;
use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// The version of [`SECRET_RULES`]. It changes whenever a rule is added,
/// removed or changed.
pub const SECRET_RULES_VERSION: u32 = 1;

/// One known secret format: a name, a regex, and an example it should match
/// and one it shouldn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecretRule {
    name: &'static str,
    pattern: &'static str,
    example: &'static str,
    counterexample: &'static str,
}

/// The rules the secret check applies, version [`SECRET_RULES_VERSION`].
///
/// Generic patterns such as `password =` and JWTs are left out: their false
/// matches would block **Send** too often.
pub const SECRET_RULES: &[SecretRule] = &[
    PEM_PRIVATE_KEY,
    AWS_ACCESS_KEY_ID,
    GITHUB_TOKEN,
    GITLAB_PERSONAL_ACCESS_TOKEN,
    SLACK_TOKEN,
    STRIPE_LIVE_KEY,
    GOOGLE_API_KEY,
    OPENROUTER_API_KEY,
    ANTHROPIC_API_KEY,
    OPENAI_API_KEY,
];

/// A PEM private key block, from its `BEGIN` line to its `END` line.
///
/// Source: RFC 7468 section 2 (<https://www.rfc-editor.org/rfc/rfc7468>),
/// whose encapsulation boundaries are `-----BEGIN label-----` and
/// `-----END label-----`, with the labels `PRIVATE KEY` and `ENCRYPTED
/// PRIVATE KEY`. It also covers OpenSSL's older labels (`RSA PRIVATE KEY`,
/// `EC PRIVATE KEY`), OpenSSH's `OPENSSH PRIVATE KEY` (`sshkey.c`) and
/// OpenPGP's `PGP PRIVATE KEY BLOCK` armor (RFC 9580 section 6.2.1,
/// <https://www.rfc-editor.org/rfc/rfc9580>), which has the same shape.
///
/// Between the boundaries, the block holds no quote and no run of dashes:
/// that finds a key in a diff, in YAML or in JSON with escaped newlines, but
/// not code that only names the boundaries, and never runs from an
/// unfinished block into the next one. The `END` label isn't checked against
/// the `BEGIN` label, because `regex` has no backreferences.
#[expect(
    clippy::doc_markdown,
    reason = "OpenSSH and OpenPGP are names, not code"
)]
const PEM_PRIVATE_KEY: SecretRule = SecretRule {
    name: "PEM private key",
    // A removed line in a diff puts one more dash before `-----END`.
    pattern: "-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----\
              (?:[^-\"'`]|-[^-\"'`])*?\
              -?-----END (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
    example: concat!(
        "-----BEGIN ",
        "PRIVATE KEY-----\nFAKE\n-----END ",
        "PRIVATE KEY-----"
    ),
    counterexample: concat!(
        "-----BEGIN ",
        "PUBLIC KEY-----\nFAKE\n-----END ",
        "PUBLIC KEY-----"
    ),
};

/// An AWS access key ID: `AKIA` (long-term) or `ASIA` (temporary, from STS)
/// and 16 more uppercase letters and digits.
///
/// Source: AWS IAM User Guide, "IAM identifiers"
/// (<https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html>),
/// whose table of unique ID prefixes lists `AKIA` for access keys and `ASIA`
/// for temporary ones. The IAM API reference allows any 16 to 128 word
/// characters (<https://docs.aws.amazon.com/IAM/latest/APIReference/API_AccessKey.html>);
/// the 20 characters here fit AWS's example key ID and gitleaks' rule.
const AWS_ACCESS_KEY_ID: SecretRule = SecretRule {
    name: "AWS access key ID",
    pattern: r"(?-u:\b)(?:AKIA|ASIA)[A-Z0-9]{16}(?-u:\b)",
    example: concat!("AKIA", "FAKEFAKEFAKEFAKE"),
    counterexample: concat!("AKIA", "FAKEFAKE"),
};

/// A GitHub token: `ghp_` (personal access token), `gho_` (OAuth), `ghu_`
/// (user-to-server), `ghs_` (server-to-server), `ghr_` (refresh) or
/// `github_pat_` (fine-grained personal access token).
///
/// Source: GitHub Docs, "About authentication to GitHub"
/// (<https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/about-authentication-to-github>),
/// section "GitHub's token formats", which lists the prefixes, and the GitHub
/// blog post "Behind GitHub's new authentication token formats" (2021): 30
/// random Base62 characters and a 6-character checksum after the prefix.
/// Tokens may grow to 255 characters, and since April 2026 installation
/// tokens (`ghs_`) can be about 520 characters with `.` and `-` (GitHub
/// changelog, 24 April 2026), so no upper bound is set. The 82-character
/// body of `github_pat_` follows gitleaks, because GitHub doesn't document
/// it.
const GITHUB_TOKEN: SecretRule = SecretRule {
    name: "GitHub token",
    pattern: r"(?-u:\b)(?:gh[pour]_[A-Za-z0-9_]{36,}|ghs_[A-Za-z0-9_.-]{36,}|github_pat_[A-Za-z0-9_]{82,})",
    example: concat!("ghp_", "FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAKE"),
    counterexample: concat!("ghp_", "FAKEfakeFAKE"),
};

/// A GitLab personal access token: `glpat-` and at least 20 characters.
///
/// Source: GitLab Docs, "Token prefixes" in
/// <https://docs.gitlab.com/security/tokens/> (`glpat-` for personal,
/// project, group and impersonation tokens), and
/// <https://docs.gitlab.com/user/profile/personal_access_tokens/>, which says
/// the token is 20 characters long. The newer routable tokens (GitLab
/// handbook, "Routable tokens") carry a longer Base64 payload after the same
/// prefix, which this also matches.
const GITLAB_PERSONAL_ACCESS_TOKEN: SecretRule = SecretRule {
    name: "GitLab personal access token",
    pattern: r"(?-u:\b)glpat-[0-9A-Za-z_-]{20,}",
    example: concat!("glpat-", "FAKEfakeFAKEfakeFAKE"),
    counterexample: concat!("glpat-", "FAKEfake"),
};

/// A Slack token: `xoxb-` (bot), `xoxp-` (user), `xapp-` (app-level),
/// `xwfp-` (workflow) or another `xox?-` type, optionally with the `xoxe.`
/// prefix of a rotating token.
///
/// Source: Slack API docs, <https://docs.slack.dev/authentication/tokens>
/// and <https://docs.slack.dev/authentication/using-token-rotation>, which
/// list the prefixes and ask integrators to expect up to 255 characters and
/// not to rely on any other structure. The older `xoxa-`, `xoxo-`, `xoxr-`
/// and `xoxs-` types follow gitleaks.
const SLACK_TOKEN: SecretRule = SecretRule {
    name: "Slack token",
    pattern: r"(?-u:\b)(?:xoxe\.)?(?:xox[abeoprs]|xapp|xwfp)-[0-9A-Za-z-]{10,}",
    example: concat!("xoxb-", "FAKEFAKEFA"),
    counterexample: concat!("xoxb-", "FAKE"),
};

/// A Stripe live secret key (`sk_live_`) or restricted key (`rk_live_`).
/// Test-mode keys (`_test_`) are left out: they can't move money.
///
/// Source: Stripe Docs, "API keys" (<https://docs.stripe.com/keys>), which
/// gives the prefixes. Stripe documents no length; the 10-character minimum
/// follows gitleaks.
const STRIPE_LIVE_KEY: SecretRule = SecretRule {
    name: "Stripe live key",
    pattern: r"(?-u:\b)[rs]k_live_[0-9A-Za-z]{10,}",
    example: concat!("sk_live_", "FAKEFAKEFA"),
    counterexample: concat!("sk_test_", "FAKEFAKEFA"),
};

/// A Google API key: `AIza` and 35 letters, digits, `-` or `_`.
///
/// Source: Google Cloud docs
/// (<https://cloud.google.com/docs/authentication/api-keys>), whose example
/// key starts with `AIza`. Google publishes no length; the 39-character
/// format follows Microsoft Purview's "Google API key" sensitive information
/// type.
const GOOGLE_API_KEY: SecretRule = SecretRule {
    name: "Google API key",
    pattern: r"(?-u:\b)AIza[0-9A-Za-z_-]{35}",
    example: concat!("AIza", "FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAK"),
    counterexample: concat!("AIza", "FAKEfake"),
};

/// An OpenRouter API key: `sk-or-v1-` and 64 lowercase hexadecimal digits.
///
/// Source: OpenRouter docs
/// (<https://openrouter.ai/docs/guides/features/guardrails/secret-formats>),
/// which gives this format.
#[expect(clippy::doc_markdown, reason = "OpenRouter is a name, not code")]
const OPENROUTER_API_KEY: SecretRule = SecretRule {
    name: "OpenRouter API key",
    pattern: r"(?-u:\b)sk-or-v1-[0-9a-f]{64}",
    example: concat!(
        "sk-or-v1-",
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    ),
    counterexample: concat!("sk-or-v1-", "deadbeef"),
};

/// An Anthropic API key (`sk-ant-api03-`), Admin API key (`sk-ant-admin01-`)
/// or other `sk-ant-` key with a type and version.
///
/// Source: Claude Docs, <https://platform.claude.com/docs/en/get-api-key> and
/// <https://platform.claude.com/docs/en/manage-claude/admin-api-keys>, which
/// give the prefixes. Anthropic documents no length: keys seen so far have 95
/// characters after the prefix, so a minimum of 32 keeps a shorter future
/// format from slipping through.
const ANTHROPIC_API_KEY: SecretRule = SecretRule {
    name: "Anthropic API key",
    pattern: r"(?-u:\b)sk-ant-[a-z]{3,5}[0-9]{2}-[A-Za-z0-9_-]{32,}",
    example: concat!("sk-ant-", "api03-", "FAKEfakeFAKEfakeFAKEfakeFAKEfake"),
    counterexample: concat!("sk-ant-", "api03-", "FAKEfake"),
};

/// An OpenAI API key: `sk-`, `sk-proj-`, `sk-svcacct-` or `sk-admin-`, with
/// the marker `T3BlbkFJ` (Base64 for "OpenAI") in the middle.
///
/// Source: OpenAI publishes no key format. The prefixes are those its
/// dashboard issues, and the marker follows gitleaks' `openai-api-key` rule
/// (<https://github.com/gitleaks/gitleaks/blob/master/config/gitleaks.toml>).
/// A key without the marker is missed.
#[expect(clippy::doc_markdown, reason = "OpenAI is a name, not code")]
const OPENAI_API_KEY: SecretRule = SecretRule {
    name: "OpenAI API key",
    pattern: r"(?-u:\b)sk-(?:(?:proj|svcacct|admin)-)?[A-Za-z0-9_-]{20,}T3BlbkFJ[A-Za-z0-9_-]{20,}",
    example: concat!(
        "sk-",
        "FAKEfakeFAKEfakeFAKE",
        "T3Blbk",
        "FJ",
        "FAKEfakeFAKEfakeFAKE"
    ),
    counterexample: concat!("sk-", "FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAKEfake"),
};

impl SecretRule {
    /// The rule's name, which its findings show.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The regex the rule matches, in `regex` crate syntax.
    #[must_use]
    pub fn pattern(&self) -> &'static str {
        self.pattern
    }

    /// A fake secret the rule should find.
    #[must_use]
    pub fn example(&self) -> &'static str {
        self.example
    }

    /// A text the rule should find no secret in.
    #[must_use]
    pub fn counterexample(&self) -> &'static str {
        self.counterexample
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

/// [`SECRET_RULES`], compiled.
static COMPILED_RULES: LazyLock<Vec<(&SecretRule, Regex)>> = LazyLock::new(|| {
    SECRET_RULES
        .iter()
        .map(|rule| {
            let regex = Regex::new(rule.pattern).expect("every secret rule is a valid regex");
            (rule, regex)
        })
        .collect()
});

/// The possible secrets in `text`, in reading order. Matches that start at
/// the same place keep the order of [`SECRET_RULES`].
#[must_use]
pub fn find_secrets(text: &str) -> Vec<SecretFinding> {
    let mut matches: Vec<_> = COMPILED_RULES
        .iter()
        .flat_map(|(rule, regex)| regex.find_iter(text).map(move |found| (rule, found)))
        .collect();
    matches.sort_by_key(|(_, found)| found.start());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str) -> &'static SecretRule {
        SECRET_RULES
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
    fn the_first_rule_set_covers_the_known_secret_formats() {
        let names: Vec<_> = SECRET_RULES.iter().map(SecretRule::name).collect();

        assert_eq!(SECRET_RULES_VERSION, 1);
        assert_eq!(
            names,
            [
                "PEM private key",
                "AWS access key ID",
                "GitHub token",
                "GitLab personal access token",
                "Slack token",
                "Stripe live key",
                "Google API key",
                "OpenRouter API key",
                "Anthropic API key",
                "OpenAI API key",
            ]
        );
    }

    #[test]
    fn every_rule_finds_its_example_and_not_its_counterexample() {
        for rule in SECRET_RULES {
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
