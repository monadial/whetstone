//! Where a TOML document goes wrong, as area files and the repository
//! config report it.

/// The 1-based line of `text` that `error` points at, and what it says.
pub(crate) fn line_and_message(text: &str, error: &toml::de::Error) -> (usize, String) {
    let start = error.span().map_or(0, |span| span.start);
    let line = 1 + text
        .bytes()
        .take(start)
        .filter(|&byte| byte == b'\n')
        .count();
    (line, error.message().trim().to_owned())
}
