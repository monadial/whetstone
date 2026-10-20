//! Connected code in the outbound review: the callers and callees of the
//! callables a change adds or modifies, found in the background once the
//! review opens, and listed, read and left out like touched files.

use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, columns};

/// From home: open the repository's picker, choose the newest commit, and
/// open its outbound review, with the five default areas selected.
const REVIEW_NEWEST: &str = "<Enter> j j j j j <Space> k k k k k <S-Tab> <Enter>";

/// The Connected code section: its summary, then its rows, trimmed, up to
/// the next blank row.
fn connected_code(session: &Session) -> Vec<String> {
    let rows = session.rows();
    let start = rows
        .iter()
        .position(|row| row.trim().starts_with("Connected code"));
    assert!(start.is_some(), "no Connected code:\n{}", session.text());
    rows.iter()
        .skip(start.unwrap_or_default())
        .map(|row| row.trim().to_owned())
        .take_while(|row| !row.is_empty())
        .collect()
}

/// The Connected code section's rows after its summary, split into their
/// columns, without the focus marker.
fn connected_rows(session: &Session) -> Vec<Vec<String>> {
    columns(&connected_code(session)[1..])
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|column| column.trim_start_matches("> ").to_owned())
                .collect()
        })
        .collect()
}

/// A Rust crate called `shop` in which `Payments::charge` validates with
/// `check::validate` and is called by `checkout`.
fn rust_payments() -> TestRepo {
    let repo = TestRepo::new("shop");
    repo.write("src/lib.rs", "mod check;\nmod pay;\nmod shop;\n")
        .write(
            "src/check.rs",
            "pub fn validate(amount: u32) -> bool {\n    amount > 0\n}\n",
        )
        .write("src/pay.rs", rust_pay("validate(amount)"))
        .write(
            "src/shop.rs",
            "use crate::pay::Payments;\n\
             \n\
             pub fn checkout(payments: &Payments) -> bool {\n\
             \x20   payments.charge(10)\n\
             }\n",
        );
    repo.commit("Start the shop");
    repo
}

/// `src/pay.rs`, whose `Payments::charge` returns `body`.
fn rust_pay(body: &str) -> String {
    format!(
        "use crate::check::validate;\n\
         \n\
         pub struct Payments;\n\
         \n\
         impl Payments {{\n\
         \x20   pub fn charge(&self, amount: u32) -> bool {{\n\
         \x20       {body}\n\
         \x20   }}\n\
         }}\n"
    )
}

#[test]
fn a_rust_change_lists_the_callers_and_callees_of_the_method_it_modifies_once_found() {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    repo.commit("Double the charge");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(connected_code(&session), ["Connected code: finding…"]);

    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB"
    );
    assert_eq!(
        connected_rows(&session),
        [
            [
                "src/check.rs",
                "lines 1–3",
                "1 KB",
                "included",
                "called by `charge`"
            ],
            [
                "src/shop.rs",
                "lines 3–5",
                "1 KB",
                "included",
                "calls `charge`"
            ],
        ]
    );
}
