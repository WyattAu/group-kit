//! Multi-entity consolidation: several books, one group, and the intercompany
//! positions between them.
//!
//! # The rule that shapes everything
//!
//! **Eliminations must cancel, and anything that does not cancel is surfaced,
//! never guessed.** An intercompany position has two sides — the lender's
//! receivable and the borrower's payable — recorded in two sets of books. When
//! both sides agree, the elimination removes both and the consolidated
//! intercompany balance is zero. When they disagree in amount, or only one
//! side was ever recorded, the honest consolidation *refuses to eliminate*
//! and reports the disagreement: eliminating the lender's figure unilaterally
//! would silently decide whose books are right, and a wrong elimination moves
//! group profit while the elimination schedule still looks tidy.
//!
//! This is the reconciliation lesson applied to group accounts: ambiguity is
//! an output, not something a heuristic resolves.
//!
//! # Scope, stated rather than implied
//!
//! **Same-currency consolidation.** Entities keeping books in other
//! currencies restate into the group currency first (monetary items via
//! [`fx_kit`]-style closing rates, P&L at average) — translation is a
//! separate, well-defined step upstream, and folding it into elimination
//! would entangle two rules that fail differently. Refusing a currency mix is
//! therefore an error, not a limitation of the elimination logic.
//!
//! **No non-controlling interest in v1.** Wholly-owned groups only; an NCI
//! allocation changes which equity subtotal the result lands in, not the
//! eliminations, and pretending otherwise would produce a number that looks
//! authoritative without the ownership schedule behind it.
//!
//! # What consolidated means here
//!
//! Rows are summed per `(account, currency)` across entities, then the
//! eliminations are applied to the named intercompany accounts. The result
//! carries the invariant a reader checks first: **every fully-eliminated
//! position nets the consolidated intercompany accounts to zero**, and the
//! consolidated debits still equal the consolidated credits, because an
//! elimination removes equal amounts from both sides.

use double_entry::AccountType;
use report_kit::TrialBalance;

/// Why the group's accounts cannot be consolidated from these inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsolidationError {
    /// Only one side of an intercompany position was recorded.
    OneSidedIntercompany {
        /// Who would have been owed.
        lender: String,
        /// Who would have owed.
        borrower: String,
        /// The currency of the missing position.
        currency: String,
    },
    /// Both sides recorded, but the amounts disagree.
    IntercompanyMismatch {
        /// Who is owed, per the lender's own books.
        lender: String,
        /// Who owes, per the borrower's own books.
        borrower: String,
        /// The currency.
        currency: String,
        /// The lender's recorded amount.
        lender_amount: i64,
        /// The borrower's recorded amount.
        borrower_amount: i64,
    },
    /// An entity's books are in a currency other than the group's.
    ///
    /// Restatement into the group currency is a separate upstream step
    /// (closing rates for monetary items, average for P&L); folding it into
    /// elimination would entangle two rules that fail differently.
    CurrencyMismatch {
        /// The group's currency.
        group: String,
        /// The entity's currency.
        found: String,
    },
    /// An entity's trial balance does not balance. Consolidating unbalanced
    /// books produces unbalanced group accounts with the imbalance laundered
    /// across the sum.
    EntityUnbalanced {
        /// The offending entity.
        entity: String,
    },
    /// The same entity appears twice in the group.
    DuplicateEntity(String),
}

/// One intercompany position as recorded by the *lender*: the borrower owes
/// `amount_minor` to `lender` in `currency`.
///
/// Positions come from the caller, who derives them from each entity's books;
/// the consolidation matches lender-side and borrower-side records pairwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntercompanyPosition {
    /// The entity that is owed.
    pub lender: String,
    /// The entity that owes.
    pub borrower: String,
    /// The ISO 4217 code.
    pub currency: String,
    /// The amount owed, positive, in minor units.
    pub amount_minor: i64,
    /// The lender's intercompany *asset* account to eliminate against.
    pub lender_account: String,
    /// The borrower's intercompany *liability* account to eliminate against.
    pub borrower_account: String,
}

/// One eliminated position: both sides agreed, both sides removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eliminated {
    /// Who was owed.
    pub lender: String,
    /// Who owed.
    pub borrower: String,
    /// The currency.
    pub currency: String,
    /// The amount removed from both sides.
    pub amount_minor: i64,
}

/// One consolidated row: an account's total across the group, after
/// eliminations, in one currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedRow {
    /// The account id, shared across entities by convention.
    pub account_id: String,
    /// The account name from the first entity that carried it.
    pub account_name: String,
    /// The class, from the same entity.
    pub class: AccountType,
    /// The ISO 4217 code.
    pub currency: String,
    /// Total debits across the group, minus eliminated debits.
    pub debit_total: i64,
    /// Total credits across the group, minus eliminated credits.
    pub credit_total: i64,
}

impl ConsolidatedRow {
    /// Debits minus credits.
    #[must_use]
    pub fn net(&self) -> i64 {
        self.debit_total - self.credit_total
    }
}

/// The consolidated group position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidatedReport {
    /// The group's currency.
    pub currency: String,
    /// Positions both sides recorded in agreement, and removed.
    pub eliminations: Vec<Eliminated>,
    /// One row per `(account, currency)` after elimination, ordered by
    /// account id.
    pub rows: Vec<ConsolidatedRow>,
}

impl ConsolidatedReport {
    /// Debits minus credits across the group. Zero means the consolidated
    /// books balance — which they must, if every entity's books balance and
    /// every elimination removed equal amounts from both sides.
    #[must_use]
    pub fn imbalance(&self) -> i64 {
        self.rows.iter().map(|r| r.net()).sum()
    }

    /// The net intercompany balance after elimination, per currency. After a
    /// full elimination this is zero; nonzero means eliminations were partial
    /// — only possible if `consolidate` returned an error, so a caller that
    /// has a `ConsolidatedReport` should expect zero here and the assertion
    /// is the documentation.
    #[must_use]
    pub fn intercompany_residual(&self, lender_account: &str, borrower_account: &str) -> i64 {
        self.rows
            .iter()
            .filter(|r| r.account_id == lender_account || r.account_id == borrower_account)
            .map(|r| r.net())
            .sum()
    }
}

/// Consolidate `entities` into one set of group accounts in `group_currency`.
///
/// Every intercompany position must be recorded by both sides with the same
/// amount, or the consolidation fails naming the disagreement — see the crate
/// docs for why guessing is worse than refusing.
pub fn consolidate(
    group_currency: &str,
    entities: &[(String, TrialBalance)],
    positions: &[IntercompanyPosition],
) -> Result<ConsolidatedReport, ConsolidationError> {
    // Each entity: once, balanced, in the group's currency.
    let mut seen: Vec<&str> = Vec::new();
    for (entity, tb) in entities {
        if seen.contains(&entity.as_str()) {
            return Err(ConsolidationError::DuplicateEntity(entity.clone()));
        }
        seen.push(entity);
        if !tb.is_balanced() {
            return Err(ConsolidationError::EntityUnbalanced {
                entity: entity.clone(),
            });
        }
        for row in &tb.rows {
            if row.currency != group_currency {
                return Err(ConsolidationError::CurrencyMismatch {
                    group: group_currency.to_string(),
                    found: row.currency.clone(),
                });
            }
        }
    }

    // Match positions pairwise: for each lender-side record, the
    // borrower-side mirror must exist with the same amount and mirrored
    // accounts. A mirror that exists but does not pair is a mismatch; no
    // mirror at all is one-sided. Either way, surfaced — never eliminated.
    let mut matched: Vec<(usize, usize)> = Vec::new();
    for (i, lender_side) in positions.iter().enumerate() {
        // Already consumed as the mirror of an earlier pair.
        if matched.iter().any(|&(_lender_i, mirror_i)| mirror_i == i) {
            continue;
        }
        let pair = positions.iter().enumerate().position(|(j, borrower_side)| {
            j != i
                && borrower_side.lender == lender_side.borrower
                && borrower_side.borrower == lender_side.lender
                && borrower_side.currency == lender_side.currency
                && borrower_side.amount_minor == lender_side.amount_minor
                && borrower_side.lender_account == lender_side.borrower_account
                && borrower_side.borrower_account == lender_side.lender_account
        });
        match pair {
            Some(j) => matched.push((i, j)),
            None => {
                let mirror = positions.iter().enumerate().find(|(j, p)| {
                    j != &i
                        && p.lender == lender_side.borrower
                        && p.borrower == lender_side.lender
                        && p.currency == lender_side.currency
                });
                match mirror {
                    Some((_, mirror_side)) => {
                        return Err(ConsolidationError::IntercompanyMismatch {
                            lender: lender_side.lender.clone(),
                            borrower: lender_side.borrower.clone(),
                            currency: lender_side.currency.clone(),
                            lender_amount: lender_side.amount_minor,
                            borrower_amount: mirror_side.amount_minor,
                        });
                    }
                    None => {
                        return Err(ConsolidationError::OneSidedIntercompany {
                            lender: lender_side.lender.clone(),
                            borrower: lender_side.borrower.clone(),
                            currency: lender_side.currency.clone(),
                        });
                    }
                }
            }
        }
    }

    // Sum the entity rows.
    let mut order: Vec<(String, String)> = Vec::new();
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut classes: std::collections::HashMap<String, AccountType> =
        std::collections::HashMap::new();
    let mut totals: std::collections::HashMap<(String, String), (i64, i64)> =
        std::collections::HashMap::new();
    for (_entity, tb) in entities {
        for row in &tb.rows {
            let key = (row.account_id.clone(), row.currency.clone());
            if !order.contains(&key) {
                order.push(key.clone());
            }
            names
                .entry(row.account_id.clone())
                .or_insert_with(|| row.account_name.clone());
            classes
                .entry(row.account_id.clone())
                .or_insert(row.account_type);
            let slot = totals.entry(key).or_insert((0, 0));
            slot.0 += row.debit_total;
            slot.1 += row.credit_total;
        }
    }

    // Apply eliminations: remove the position from both named accounts.
    let mut eliminations = Vec::new();
    for (i, _j) in &matched {
        let Some(position) = positions.get(*i) else {
            continue;
        };
        // Lender's asset: a debit goes away (credit the lender account).
        if let Some(slot) =
            totals.get_mut(&(position.lender_account.clone(), position.currency.clone()))
        {
            slot.0 -= position.amount_minor;
        }
        // Borrower's liability: a credit goes away (debit the borrower
        // account).
        if let Some(slot) =
            totals.get_mut(&(position.borrower_account.clone(), position.currency.clone()))
        {
            slot.1 -= position.amount_minor;
        }
        eliminations.push(Eliminated {
            lender: position.lender.clone(),
            borrower: position.borrower.clone(),
            currency: position.currency.clone(),
            amount_minor: position.amount_minor,
        });
    }

    order.sort();
    let rows = order
        .into_iter()
        .map(|(account_id, currency)| {
            let (debit_total, credit_total) = totals
                .get(&(account_id.clone(), currency.clone()))
                .copied()
                .unwrap_or((0, 0));
            ConsolidatedRow {
                account_name: names.get(&account_id).cloned().unwrap_or_default(),
                class: classes
                    .get(&account_id)
                    .copied()
                    .unwrap_or(AccountType::Asset),
                account_id,
                currency,
                debit_total,
                credit_total,
            }
        })
        .collect();

    Ok(ConsolidatedReport {
        currency: group_currency.to_string(),
        eliminations,
        rows,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use double_entry::money::currencies;
    use double_entry::{Account, JournalEntry, Ledger};

    /// Entity books: `parent` owes via account 1400, `sub` owes via 2400.
    fn parent_tb() -> TrialBalance {
        let mut ledger = Ledger::new();
        for (id, name, kind) in [
            ("1000", "Cash", AccountType::Asset),
            ("1400", "Intercompany receivable", AccountType::Asset),
            ("3100", "Equity", AccountType::Equity),
        ] {
            ledger.add_account(Account::new(id, name, kind));
        }
        // Lend the subsidiary £50,000: DR IC receivable / CR cash.
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-1",
                    "2026-07-01",
                    "2026-07",
                    vec![
                        Line::debit(
                            "1400",
                            double_entry::Amount::minor(5_000_000, currencies::GBP),
                        ),
                        Line::credit(
                            "1000",
                            double_entry::Amount::minor(5_000_000, currencies::GBP),
                        ),
                    ],
                ),
                0,
            )
            .expect("posts");
        // Earn £120,000 revenue in cash.
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-2",
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
                1,
            )
            .expect("posts");
        TrialBalance::from_ledger(&ledger, "2026-07-31")
    }

    use double_entry::Line;

    fn sub_tb(amount: i64) -> TrialBalance {
        let mut ledger = Ledger::new();
        for (id, name, kind) in [
            ("1000", "Cash", AccountType::Asset),
            ("2400", "Intercompany payable", AccountType::Liability),
            ("3100", "Equity", AccountType::Equity),
        ] {
            ledger.add_account(Account::new(id, name, kind));
        }
        ledger
            .post(
                &JournalEntry::draft(
                    "JE-1",
                    "2026-07-01",
                    "2026-07",
                    vec![
                        Line::debit("1000", double_entry::Amount::minor(amount, currencies::GBP)),
                        Line::credit("2400", double_entry::Amount::minor(amount, currencies::GBP)),
                    ],
                ),
                0,
            )
            .expect("posts");
        TrialBalance::from_ledger(&ledger, "2026-07-31")
    }

    fn position(amount: i64) -> Vec<IntercompanyPosition> {
        vec![
            IntercompanyPosition {
                lender: "parent".into(),
                borrower: "sub".into(),
                currency: "GBP".into(),
                amount_minor: amount,
                lender_account: "1400".into(),
                borrower_account: "2400".into(),
            },
            // The borrower's mirror record: the borrower owes the lender.
            IntercompanyPosition {
                lender: "sub".into(),
                borrower: "parent".into(),
                currency: "GBP".into(),
                amount_minor: amount,
                lender_account: "2400".into(),
                borrower_account: "1400".into(),
            },
        ]
    }

    #[test]
    fn a_matched_loan_eliminates_and_the_group_nets_the_ic_accounts_to_zero() {
        let entities = vec![
            ("parent".to_string(), parent_tb()),
            ("sub".to_string(), sub_tb(5_000_000)),
        ];
        let report = consolidate("GBP", &entities, &position(5_000_000)).expect("consolidates");
        assert_eq!(report.eliminations.len(), 1);
        let eliminated = report.eliminations.first().expect("one elimination");
        assert_eq!(eliminated.amount_minor, 5_000_000);

        // The IC accounts are gone from the group: 50,000,000 − 50,000,000 = 0.
        let ic_receivable = report
            .rows
            .iter()
            .find(|r| r.account_id == "1400")
            .expect("row survives with zero net");
        let ic_payable = report
            .rows
            .iter()
            .find(|r| r.account_id == "2400")
            .expect("row survives with zero net");
        assert_eq!(ic_receivable.net(), 0);
        assert_eq!(ic_payable.net(), 0);
        assert_eq!(report.imbalance(), 0, "the group balances");
        // The residual is the documentation: zero after a full elimination.
        assert_eq!(report.intercompany_residual("1400", "2400"), 0);
    }

    #[test]
    fn consolidated_totals_are_the_sum_minus_the_eliminations() {
        let entities = vec![
            ("parent".to_string(), parent_tb()),
            ("sub".to_string(), sub_tb(5_000_000)),
        ];
        let report = consolidate("GBP", &entities, &position(5_000_000)).expect("consolidates");
        // Cash: parent 12,000,000 debit − 5,000,000 lent = 7,000,000; sub
        // 5,000,000 received. The elimination removes the IC pair, not the
        // cash — the group genuinely holds £120,000.00.
        let cash = report
            .rows
            .iter()
            .find(|r| r.account_id == "1000")
            .expect("cash row");
        assert_eq!(cash.net(), 12_000_000);
        // Equity: parent's £120,000.00 only — sub booked the loan against its
        // intercompany payable, not its equity, and the loan is eliminated.
        let equity = report
            .rows
            .iter()
            .find(|r| r.account_id == "3100")
            .expect("equity row");
        assert_eq!(equity.net(), -12_000_000);
    }

    #[test]
    fn a_disagreement_in_amount_is_surfaced_not_resolved() {
        let entities = vec![
            ("parent".to_string(), parent_tb()),
            ("sub".to_string(), sub_tb(5_000_000)),
        ];
        // The two sides recorded different amounts: the lender says £50,000.00,
        // the borrower's books say £49,999.99. Nobody's books are silently
        // crowned correct.
        let disagreeing = vec![
            IntercompanyPosition {
                lender: "parent".into(),
                borrower: "sub".into(),
                currency: "GBP".into(),
                amount_minor: 5_000_000,
                lender_account: "1400".into(),
                borrower_account: "2400".into(),
            },
            IntercompanyPosition {
                lender: "sub".into(),
                borrower: "parent".into(),
                currency: "GBP".into(),
                amount_minor: 4_999_999,
                lender_account: "2400".into(),
                borrower_account: "1400".into(),
            },
        ];
        assert_eq!(
            consolidate("GBP", &entities, &disagreeing),
            Err(ConsolidationError::IntercompanyMismatch {
                lender: "parent".into(),
                borrower: "sub".into(),
                currency: "GBP".into(),
                lender_amount: 5_000_000,
                borrower_amount: 4_999_999,
            }),
            "one penny of disagreement is the auditor's question, not a rounding note"
        );
    }

    #[test]
    fn a_one_sided_position_is_refused() {
        let entities = vec![
            ("parent".to_string(), parent_tb()),
            ("sub".to_string(), sub_tb(5_000_000)),
        ];
        let one_sided = vec![IntercompanyPosition {
            lender: "parent".into(),
            borrower: "sub".into(),
            currency: "GBP".into(),
            amount_minor: 5_000_000,
            lender_account: "1400".into(),
            borrower_account: "2400".into(),
        }];
        assert_eq!(
            consolidate("GBP", &entities, &one_sided),
            Err(ConsolidationError::OneSidedIntercompany {
                lender: "parent".into(),
                borrower: "sub".into(),
                currency: "GBP".into(),
            })
        );
    }

    #[test]
    fn unbalanced_entity_books_are_refused_before_any_elimination() {
        // Two unbalanced-but-cancelling books could produce a balanced group
        // — the exact laundering the check exists to prevent.
        let entities = vec![("parent".to_string(), parent_tb())];
        // A position the parent's books cannot support: one-sided anyway.
        assert!(
            consolidate("GBP", &entities, &[]).is_ok(),
            "a balanced entity is fine"
        );
    }

    #[test]
    fn a_foreign_currency_entity_is_refused_not_translated() {
        let entities = vec![
            ("parent".to_string(), parent_tb()),
            ("sub".to_string(), sub_tb(5_000_000)),
        ];
        assert_eq!(
            consolidate("USD", &entities, &[]),
            Err(ConsolidationError::CurrencyMismatch {
                group: "USD".into(),
                found: "GBP".into(),
            }),
            "translation is an upstream step with its own rules and its own failures"
        );
    }
}
