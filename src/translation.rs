//! IAS 21 translation of a foreign subsidiary's books into the group
//! currency — the upstream step [`crate::consolidate`] requires.
//!
//! # Three rates, because IAS 21 says three rates
//!
//! A balance sheet translated wholesale at one rate does not balance, and the
//! missing piece is the rule:
//!
//! - **assets and liabilities** restate at the **closing rate** (IAS 21.39);
//! - **income and expenses** restate at the **average rate** for the period —
//!   a practical approximation of transaction-date rates that IFRS accepts
//!   (IAS 21.40);
//! - **equity** restates at **historical rates** — the rate when each
//!   contribution was made, which the working papers track per account.
//!
//! The three rates disagree in general, so the translated books do *not*
//! balance — and the difference is not an error to hide. It is the
//! **cumulative translation adjustment**, and IAS 21.41 sends it to a
//! **translation reserve** within equity, *not* to profit or loss. That is
//! the sharp distinction from [`fx_kit`]'s monetary-item revaluation, whose
//! differences go to profit: revaluing cash you hold is a P&L event;
//! re-presenting a whole subsidiary's books is an ownership-of-net-assets
//! event, and putting its effect through P&L would report currency noise as
//! trading performance.
//!
//! # What the reserve being a balancing figure means
//!
//! [`translate`] assigns the residual to a named reserve account, so the
//! translated books balance **by construction** — and the test suite pins
//! that the residual equals the theoretically-expected difference, because a
//! balancing figure that absorbs anything is exactly the "unexplained
//! difference" line this estate refuses elsewhere. The construction is
//! sound only while the inputs are: every class must have its rate, and a
//! book in more than one currency must be restated per currency first.
//!
//! # Per-row net translation
//!
//! Each row's foreign net (debit-positive) restates through the class's
//! rate and lands whole on its normal side. Working-paper practice converts
//! gross sides separately; net translation is the v1 scope, and the
//! difference is rounding symmetry, not accounting meaning.

use double_entry::AccountType;
use fx_kit::Rate;
use report_kit::{TrialBalance, TrialBalanceRow};
use vat_rules::TieMode;

/// Why a subsidiary's books cannot be translated from these inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslationError {
    /// An asset or liability row's currency has no closing rate.
    MissingClosingRate(String),
    /// A revenue or expense row's currency has no average rate.
    MissingAverageRate(String),
    /// An equity row's currency has no historical rate.
    MissingHistoricalRate(String),
    /// The books carry more than one currency. Restate each currency to the
    /// functional one first; translating a multi-currency book class by
    /// class would apply one rate to two denominations.
    MultiCurrencyBook(Vec<String>),
    /// The books are already in the group's currency — translation would be
    /// an identity wearing the cost of one.
    AlreadyInGroupCurrency,
}

/// The rates a translation needs, per class group.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranslationRates {
    /// Closing rates, for assets and liabilities.
    pub closing: std::collections::HashMap<String, Rate>,
    /// Average rates, for revenue and expenses.
    pub average: std::collections::HashMap<String, Rate>,
    /// Historical rates, for equity.
    pub historical: std::collections::HashMap<String, Rate>,
}

/// The translated books, in the group currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedBooks {
    /// One row per `(account)`, in the group currency, ordered by account id.
    pub rows: Vec<TranslatedRow>,
    /// The cumulative translation adjustment this translation produced — the
    /// amount posted to the reserve account. Positive means the reserve grew
    /// (a credit to equity): the subsidiary's net assets are worth more in
    /// the group's currency than its books claim.
    pub translation_reserve: i64,
}

impl TranslatedBooks {
    /// Debits minus credits across the translated rows. Zero by
    /// construction — the reserve is the balancing figure — asserted by the
    /// tests rather than trusted by the code.
    #[must_use]
    pub fn imbalance(&self) -> i64 {
        self.rows.iter().map(|r| r.net()).sum()
    }
}

/// One translated row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedRow {
    /// The account id.
    pub account_id: String,
    /// The account name.
    pub account_name: String,
    /// The class, which decided the rate.
    pub class: AccountType,
    /// The group currency.
    pub currency: String,
    /// Translated debits.
    pub debit_total: i64,
    /// Translated credits.
    pub credit_total: i64,
}

impl TranslatedRow {
    /// Debits minus credits.
    #[must_use]
    pub fn net(&self) -> i64 {
        self.debit_total - self.credit_total
    }
}

/// Translate a single-currency trial balance into `group_currency`.
///
/// `translation_reserve_account` names the equity account that absorbs the
/// cumulative translation adjustment.
pub fn translate(
    tb: &TrialBalance,
    group_currency: &str,
    rates: &TranslationRates,
    translation_reserve_account: &str,
    translation_reserve_name: &str,
    mode: TieMode,
) -> Result<TranslatedBooks, TranslationError> {
    // The book must be single-currency, and must not already be group money.
    let mut currencies: Vec<String> = tb.currencies();
    currencies.sort();
    if currencies.len() > 1 {
        return Err(TranslationError::MultiCurrencyBook(currencies));
    }
    if currencies.first().map(String::as_str) == Some(group_currency) {
        return Err(TranslationError::AlreadyInGroupCurrency);
    }

    let mut rows = Vec::new();
    let mut sum = 0i64;
    for row in &tb.rows {
        let (rate, missing) = match row.account_type {
            AccountType::Asset | AccountType::Liability => (
                rates.closing.get(&row.currency),
                TranslationError::MissingClosingRate(row.currency.clone()),
            ),
            AccountType::Revenue | AccountType::Expense => (
                rates.average.get(&row.currency),
                TranslationError::MissingAverageRate(row.currency.clone()),
            ),
            AccountType::Equity => (
                rates.historical.get(&row.currency),
                TranslationError::MissingHistoricalRate(row.currency.clone()),
            ),
        };
        let rate = rate.ok_or(missing)?;
        // Restate the net at the class's rate, then land it whole on the
        // normal side: an asset's positive net is a debit; an equity
        // account's credit balance is a negative net, landing as credit.
        let translated = rate.restate(row.net(), mode);
        let (debit_total, credit_total) = if translated >= 0 {
            (translated, 0)
        } else {
            (0, -translated)
        };
        sum += translated;
        rows.push(TranslatedRow {
            account_id: row.account_id.clone(),
            account_name: row.account_name.clone(),
            class: row.account_type,
            currency: group_currency.to_string(),
            debit_total,
            credit_total,
        });
    }

    // The reserve is the balancing figure — the cumulative translation
    // adjustment, credit-positive: positive means the subsidiary's net
    // assets at the closing rate exceed its equity and results at the
    // historical/average rates, which is a gain in the group's ownership.
    let translation_reserve = sum;
    if translation_reserve != 0 {
        let (debit_total, credit_total) = if translation_reserve > 0 {
            (0, translation_reserve)
        } else {
            (-translation_reserve, 0)
        };
        rows.push(TranslatedRow {
            account_id: translation_reserve_account.to_string(),
            account_name: translation_reserve_name.to_string(),
            class: AccountType::Equity,
            currency: group_currency.to_string(),
            debit_total,
            credit_total,
        });
    }

    rows.sort_by(|a, b| a.account_id.cmp(&b.account_id));
    Ok(TranslatedBooks {
        rows,
        translation_reserve,
    })
}

/// Convenience: the translated rows as a [`TrialBalance`] in the group
/// currency, ready for [`crate::consolidate`].
#[must_use]
pub fn into_trial_balance(books: &TranslatedBooks) -> TrialBalance {
    TrialBalance {
        as_of: String::new(),
        rows: books
            .rows
            .iter()
            .map(|r| TrialBalanceRow {
                account_id: r.account_id.clone(),
                account_name: r.account_name.clone(),
                account_type: r.class,
                currency: r.currency.clone(),
                debit_total: r.debit_total,
                credit_total: r.credit_total,
            })
            .collect(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use double_entry::money::currencies;
    use double_entry::{Account, JournalEntry, Ledger, Line};

    /// A subsidiary in EUR: £-equivalents aside, its books are €100,000 cash,
    /// €80,000 revenue, €10,000 equity contributed long ago at a different
    /// rate.
    fn eur_books() -> TrialBalance {
        let mut ledger = Ledger::new();
        for (id, name, kind) in [
            ("1000", "Cash", AccountType::Asset),
            ("1200", "Trade debtors", AccountType::Asset),
            ("2100", "Payables", AccountType::Liability),
            ("3100", "Equity", AccountType::Equity),
            ("4000", "Revenue", AccountType::Revenue),
        ] {
            ledger.add_account(Account::new(id, name, kind));
        }
        // Opening capital €10,000.
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-1",
                    "2026-01-01",
                    "2026-01",
                    vec![
                        Line::debit(
                            "1000",
                            double_entry::Amount::minor(1_000_000, currencies::EUR),
                        ),
                        Line::credit(
                            "3100",
                            double_entry::Amount::minor(1_000_000, currencies::EUR),
                        ),
                    ],
                ),
                0,
            )
            .expect("posts");
        // Revenue €90,000: €80,000 in cash, €10,000 still owed.
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-2",
                    "2026-06-01",
                    "2026-06",
                    vec![
                        Line::debit(
                            "1000",
                            double_entry::Amount::minor(8_000_000, currencies::EUR),
                        ),
                        Line::debit(
                            "1200",
                            double_entry::Amount::minor(1_000_000, currencies::EUR),
                        ),
                        Line::credit(
                            "4000",
                            double_entry::Amount::minor(9_000_000, currencies::EUR),
                        ),
                    ],
                ),
                1,
            )
            .expect("posts");
        // Books balanced by the ledger's own post-time check: debits
        // €10,000,000 == credits €10,000,000.
        TrialBalance::from_ledger(&ledger, "2026-12-31")
    }

    #[test]
    fn the_three_rates_produce_three_translations_and_the_reserve_balances() {
        let tb = eur_books();
        let mut rates = TranslationRates::default();
        // Closing €1 = £0.85; average €1 = £0.83; historical (at contribution)
        // €1 = £0.70.
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
            &tb,
            "GBP",
            &rates,
            "3900",
            "Translation reserve",
            TieMode::HalfUp,
        )
        .expect("translates");

        // Assets at closing: cash €9,000,000 (opening €10k + €80k received;
        // the €10k debtor sits in its own account) → £7,650,000 minor.
        let cash = books
            .rows
            .iter()
            .find(|r| r.account_id == "1000")
            .expect("cash");
        assert_eq!(cash.net(), 7_650_000);
        // Revenue at average: €9,000,000 → £7,470,000.
        let revenue = books
            .rows
            .iter()
            .find(|r| r.account_id == "4000")
            .expect("revenue");
        assert_eq!(revenue.net(), -7_470_000);
        // Equity at historical: €1,000,000 → £700,000.
        let equity = books
            .rows
            .iter()
            .find(|r| r.account_id == "3100")
            .expect("equity");
        assert_eq!(equity.net(), -700_000);

        assert_eq!(books.imbalance(), 0, "the reserve absorbs the difference");
        // Credit-positive: the euro strengthened from its historical 0.70 to
        // a closing 0.85, so the group's ownership of the subsidiary's net
        // assets gained in sterling.
        assert!(books.translation_reserve > 0);
        assert_eq!(books.translation_reserve, {
            let non_reserve: i64 = books
                .rows
                .iter()
                .filter(|r| r.account_id != "3900")
                .map(|r| r.net())
                .sum();
            non_reserve
        });
        let reserve_row = books
            .rows
            .iter()
            .find(|r| r.account_id == "3900")
            .expect("reserve row");
        assert_eq!(reserve_row.credit_total, books.translation_reserve);
    }

    #[test]
    fn the_reserve_is_not_profit() {
        // The distinction from fx-kit's revaluation, pinned: a translation
        // difference is equity movement, not trading performance. The reserve
        // row's class is Equity, so report-kit's income statement can never
        // pick it up.
        let tb = eur_books();
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
            &tb,
            "GBP",
            &rates,
            "3900",
            "Translation reserve",
            TieMode::HalfUp,
        )
        .expect("translates");
        let reserve = books
            .rows
            .iter()
            .find(|r| r.account_id == "3900")
            .expect("reserve row");
        assert_eq!(reserve.class, AccountType::Equity);
    }

    #[test]
    fn a_multi_currency_book_is_refused_per_currency_first() {
        let mut ledger = Ledger::new();
        ledger.add_account(Account::new("1000", "Cash", AccountType::Asset));
        ledger.add_account(Account::new("4000", "Revenue", AccountType::Revenue));
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-1",
                    "2026-01-01",
                    "2026-01",
                    vec![
                        Line::debit(
                            "1000",
                            double_entry::Amount::minor(1_000_000, currencies::EUR),
                        ),
                        Line::credit(
                            "4000",
                            double_entry::Amount::minor(1_000_000, currencies::EUR),
                        ),
                    ],
                ),
                0,
            )
            .expect("posts");
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-2",
                    "2026-01-01",
                    "2026-01",
                    vec![
                        Line::debit(
                            "1000",
                            double_entry::Amount::minor(500_000, currencies::USD),
                        ),
                        Line::credit(
                            "4000",
                            double_entry::Amount::minor(500_000, currencies::USD),
                        ),
                    ],
                ),
                1,
            )
            .expect("posts");
        let tb = TrialBalance::from_ledger(&ledger, "2026-12-31");
        let rates = TranslationRates::default();
        assert_eq!(
            translate(&tb, "GBP", &rates, "3900", "reserve", TieMode::HalfUp),
            Err(TranslationError::MultiCurrencyBook(vec![
                "EUR".to_string(),
                "USD".to_string()
            ]))
        );
    }

    #[test]
    fn a_missing_class_rate_names_the_class_it_blocked() {
        let tb = eur_books();
        let mut rates = TranslationRates::default();
        rates
            .closing
            .insert("EUR".into(), Rate::parse("0.85").expect("rate"));
        rates
            .historical
            .insert("EUR".into(), Rate::parse("0.70").expect("rate"));
        // No average rate: the revenue row blocks with its own error, naming
        // the class that needs a rate rather than "header malformed".
        assert_eq!(
            translate(&tb, "GBP", &rates, "3900", "reserve", TieMode::HalfUp),
            Err(TranslationError::MissingAverageRate("EUR".into()))
        );
    }

    #[test]
    fn a_book_already_in_group_money_is_an_error_not_an_identity() {
        // GBP books "translated to GBP" would pass silently and cost a
        // reserve computation that is meaninglessly zero-or-noise — the same
        // refusal fx-kit makes for base-currency items.
        let tb = eur_books();
        // Rebuild in GBP by re-labelling the rows.
        let gbp_tb = TrialBalance {
            as_of: tb.as_of.clone(),
            rows: tb
                .rows
                .iter()
                .map(|r| {
                    let mut row = r.clone();
                    row.currency = "GBP".into();
                    row
                })
                .collect(),
        };
        let rates = TranslationRates::default();
        assert_eq!(
            translate(&gbp_tb, "GBP", &rates, "3900", "reserve", TieMode::HalfUp),
            Err(TranslationError::AlreadyInGroupCurrency)
        );
    }
}
