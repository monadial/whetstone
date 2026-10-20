//! Cleaning pasted text before a text entry takes it (rule 57).
//!
//! Terminals commonly send a pasted line break as `\r`, and text copied on
//! Windows breaks lines with `\r\n`, so both are a line break here. Control
//! characters, other than tab and the line break, never reach what the
//! learner typed. An entry that holds one line, such as the search box,
//! also turns each line break and tab into a space; one that holds many
//! lines, such as the reasoning editor will, keeps them.

/// `pasted` as an entry that keeps line breaks takes it: each `\r\n` and
/// lone `\r` is a line break, `\n`, and every control character but tab and
/// `\n` is dropped.
pub(crate) fn clean(pasted: &str) -> String {
    pasted
        .replace("\r\n", "\n")
        .chars()
        .map(|c| if c == '\r' { '\n' } else { c })
        .filter(|&c| matches!(c, '\t' | '\n') || !c.is_control())
        .collect()
}

/// `pasted` as an entry that holds one line takes it: [`clean`], with each
/// line break and tab a space.
pub(crate) fn on_one_line(pasted: &str) -> String {
    clean(pasted)
        .chars()
        .map(|c| if matches!(c, '\t' | '\n') { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_kept_as_it_is() {
        assert_eq!(
            clean("retry, then «give up» 😀"),
            "retry, then «give up» 😀"
        );
    }

    #[test]
    fn crlf_and_a_lone_cr_are_line_breaks() {
        assert_eq!(clean("one\r\ntwo\rthree\nfour"), "one\ntwo\nthree\nfour");
    }

    #[test]
    fn a_lone_cr_before_a_crlf_is_a_line_break_of_its_own() {
        assert_eq!(clean("one\r\r\ntwo"), "one\n\ntwo");
    }

    #[test]
    fn control_characters_but_tab_and_line_breaks_are_dropped() {
        assert_eq!(
            clean("a\u{0}b\u{7}c\u{1b}[31md\u{7f}e\u{85}f\tg\nh"),
            "abc[31mdef\tg\nh"
        );
    }

    #[test]
    fn characters_that_join_or_shape_text_are_kept() {
        // A zero-width joiner, in a family emoji, and a combining accent.
        let text = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} cafe\u{301}";

        assert_eq!(clean(text), text);
    }

    #[test]
    fn on_one_line_each_line_break_and_tab_is_a_space() {
        assert_eq!(
            on_one_line("one\r\ntwo\rthree\nfour\tfive"),
            "one two three four five"
        );
    }

    #[test]
    fn on_one_line_a_trailing_line_break_is_a_trailing_space() {
        assert_eq!(on_one_line("key\n"), "key ");
    }

    #[test]
    fn on_one_line_control_characters_are_dropped_too() {
        assert_eq!(on_one_line("a\u{1b}b\u{0}\r\nc"), "ab c");
    }
}
