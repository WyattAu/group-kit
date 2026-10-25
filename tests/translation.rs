//! Translate, then consolidate: the full multi-currency group close.
//!
//! `group-kit` v1 consolidated same-currency books and refused to translate.
//! v0.2.0 adds the translation upstream step — IAS 21's three rates, with the
//! difference to a translation reserve rather than profit — so the full chain
//! is: a EUR subsidiary's books translate into GBP, the translated books then
//! consolidate against the GBP parent, and the intercompany elimination still
//! cancels.

#![allow(clippy::expect_used, clippy::panic)]

use double_entry::AccountType;
use double_entry::money::currencies;
use double_entry::{Account, JournalEntry, Ledger, Line};
use fx_kit::Rate;
use group_kit::{
    IntercompanyPosition, TranslationError, TranslationRates, consolidate, into_trial_balance,
    translate,
};
use report_kit::TrialBalance;
use vat_rules::TieMode;

/// The parent: GBP books, £50,000 lent to the subsidiary, £120,000 revenue.
fn parent_tb() -> TrialBalance {
    let mut ledger = Ledger::new();
    for (id, name, kind) in [
        ("1000", "Cash", AccountType::Asset),
        ("1400", "Intercompany receivable", AccountType::Asset),
        ("3100", "Equity", AccountType::Equity),
    ] {
        ledger.add_account(Account::new(id, name, kind));
    }
    let mut v = ledger
        .post(
            &JournalEntry::draft(
                "JE-P1",
                "2026-07-01",
                "2026-07",
                vec![
                    Line::debit(
                        "1400",
                        double_entry::Amount::minor(425_000, currencies::GBP),
                    ),
                    Line::credit(
                        "1000",
                        double_entry::Amount::minor(425_000, currencies::GBP),
                    ),
                ],
            ),
            0,
        )
        .expect("posts");
    ledger
        .post(
            &JournalEntry::draft(
                "JE-P2",
                "2026-07-02",
                "2026-07",
                vec![
                    Line::debit(
                        "1000",
                        double_entry::Amount::minor(12_000_000, currencies::GBP),
                    ),
                    Line::credit(
                        "3100",
                        double_entry::Amount::minor(12_000_000, currencies::GBP),
                    ),
                ],
            ),
            v,
        )
        .expect("posts");
    let _ = v;
    TrialBalance::from_ledger(&ledger, "2026-12-31")
}

/// The subsidiary: EUR books. €5,000 equity, €85,000 revenue (€80,000 cash,
/// €5,000 still owed by a customer), and the parent's loan received as cash
/// against its intercompany payable.
fn subsidiary_tb() -> TrialBalance {
    let mut ledger = Ledger::new();
    for (id, name, kind) in [
        ("1000", "Cash", AccountType::Asset),
        ("1200", "Trade debtors", AccountType::Asset),
        ("2400", "Intercompany payable", AccountType::Liability),
        ("3100", "Equity", AccountType::Equity),
        ("4000", "Revenue", AccountType::Revenue),
    ] {
        ledger.add_account(Account::new(id, name, kind));
    }
    // €5,000 capital.
    let mut v = ledger
        .post(
            &JournalEntry::draft(
                "JE-S1",
                "2026-07-01",
                "2026-07",
                vec![
                    Line::debit(
                        "1000",
                        double_entry::Amount::minor(500_000, currencies::EUR),
                    ),
                    Line::credit(
                        "3100",
                        double_entry::Amount::minor(500_000, currencies::EUR),
                    ),
                ],
            ),
            0,
        )
        .expect("posts");
    // The parent's loan received: €5,000 (the parent booked £4,250 at 0.85).
    v = ledger
        .post(
            &JournalEntry::draft(
                "JE-S2",
                "2026-07-01",
                "2026-07",
                vec![
                    Line::debit(
                        "1000",
                        double_entry::Amount::minor(500_000, currencies::EUR),
                    ),
                    Line::credit(
                        "2400",
                        double_entry::Amount::minor(500_000, currencies::EUR),
                    ),
                ],
            ),
            v,
        )
        .expect("posts");
    // €85,000 revenue: €80,000 collected, €5,000 owed.
    ledger
        .post(
            &JournalEntry::draft(
                "JE-S3",
                "2026-07-02",
                "2026-07",
                vec![
                    Line::debit(
                        "1000",
                        double_entry::Amount::minor(8_000_000, currencies::EUR),
                    ),
                    Line::debit(
                        "1200",
                        double_entry::Amount::minor(500_000, currencies::EUR),
                    ),
                    Line::credit(
                        "4000",
                        double_entry::Amount::minor(8_500_000, currencies::EUR),
                    ),
                ],
            ),
            v,
        )
        .expect("posts");
    TrialBalance::from_ledger(&ledger, "2026-12-31")
}

#[test]
fn translation_then_consolidation_closes_a_multi_currency_group() {
    let sub = subsidiary_tb();

    // IAS 21's three rates: closing 0.85, average 0.83, historical 0.70.
    let mut rates = TranslationRates::default();
    rates
        .closing
        .insert("EUR".into(), Rate::parse("0.85").expect("rate"));
    rates
        .average
        .insert("EUR".into(), Rate::parse("0.83").expect("rate"));
    rates
        .historical
        .insert("EUR".into(), Rate::parse("0.70").expect("rate"));

    let books = translate(
        &sub,
        "GBP",
        &rates,
        "3900",
        "Translation reserve",
        TieMode::HalfUp,
    )
    .expect("translates");
    assert_eq!(books.imbalance(), 0, "the reserve balances the translation");
    assert!(
        books.translation_reserve > 0,
        "the euro strengthened since contribution"
    );

    // Consolidate: parent + translated subsidiary, eliminating the loan.
    let entities = vec![
        ("parent".to_string(), parent_tb()),
        ("sub".to_string(), into_trial_balance(&books)),
    ];
    let positions = vec![
        IntercompanyPosition {
            lender: "parent".into(),
            borrower: "sub".into(),
            currency: "GBP".into(),
            amount_minor: 425_000,
            lender_account: "1400".into(),
            borrower_account: "2400".into(),
        },
        IntercompanyPosition {
            lender: "sub".into(),
            borrower: "parent".into(),
            currency: "GBP".into(),
            amount_minor: 425_000,
            lender_account: "2400".into(),
            borrower_account: "1400".into(),
        },
    ];
    let report = consolidate("GBP", &entities, &positions).expect("consolidates");
    assert_eq!(report.imbalance(), 0, "the group balances");
    assert_eq!(report.eliminations.len(), 1);

    // The intercompany accounts net to zero after elimination.
    assert_eq!(report.intercompany_residual("1400", "2400"), 0);

    // The translation reserve survives consolidation into equity: it is the
    // group's ownership effect, not an eliminations artefact.
    let reserve = report
        .rows
        .iter()
        .find(|r| r.account_id == "3900")
        .expect("the reserve row consolidates");
    assert_eq!(
        reserve.net(),
        -books.translation_reserve,
        "a credit, at its translated size"
    );
}

#[test]
fn an_untranslated_subsidiary_is_refused_by_the_consolidation_gate() {
    // The gate from v1 still stands: group-kit refuses the EUR books rather
    // than translating silently, which is why the translation step above is
    // explicit and the rates are visible in the working papers.
    let entities = vec![
        ("parent".to_string(), parent_tb()),
        ("sub".to_string(), subsidiary_tb()),
    ];
    assert!(matches!(
        consolidate("GBP", &entities, &[]),
        Err(group_kit::ConsolidationError::CurrencyMismatch { .. })
    ));
}

#[test]
fn translation_uses_the_class_rates_not_one_rate() {
    // The reason three rates exist: translating equity at the closing rate
    // would erase the historical contribution value, and translating revenue
    // at it would report currency movement as trading performance.
    let sub = subsidiary_tb();
    let mut closing_only = TranslationRates::default();
    closing_only
        .closing
        .insert("EUR".into(), Rate::parse("0.85").expect("rate"));
    closing_only
        .average
        .insert("EUR".into(), Rate::parse("0.85").expect("rate"));
    closing_only
        .historical
        .insert("EUR".into(), Rate::parse("0.85").expect("rate"));
    let one_rate = translate(
        &sub,
        "GBP",
        &closing_only,
        "3900",
        "reserve",
        TieMode::HalfUp,
    )
    .expect("translates");

    let mut real = TranslationRates::default();
    real.closing
        .insert("EUR".into(), Rate::parse("0.85").expect("rate"));
    real.average
        .insert("EUR".into(), Rate::parse("0.83").expect("rate"));
    real.historical
        .insert("EUR".into(), Rate::parse("0.70").expect("rate"));
    let three_rates =
        translate(&sub, "GBP", &real, "3900", "reserve", TieMode::HalfUp).expect("translates");

    // The revenue rows differ: 0.85 vs 0.83 average. Same books, different
    // rules, different numbers — which is why the rules are the API.
    let a = one_rate
        .rows
        .iter()
        .find(|r| r.account_id == "4000")
        .expect("revenue");
    let b = three_rates
        .rows
        .iter()
        .find(|r| r.account_id == "4000")
        .expect("revenue");
    assert_ne!(a.net(), b.net());
}
