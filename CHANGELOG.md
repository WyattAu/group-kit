# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [Unreleased]

## [0.2.2] - 2026-10-27

### Changed

- Tracks `fx-kit 0.1.3` — the vat-rules-convergence pin, matching the graph. No behavioural change.


## [0.2.1] - 2026-10-27

### Changed

- Tracks `vat-rules 0.1.1` — the graph's convergence pin. No behavioural change.


## [0.2.0] - 2026-10-25

### Added

- `translate` — IAS 21 translation of a foreign subsidiary's books into the
  group currency: assets and liabilities at the **closing** rate, income and
  expenses at the **average** rate, equity at **historical** rates, and the
  difference to a **translation reserve** within equity — not profit, which
  is the sharp distinction from `fx-kit`'s monetary-item revaluation.
- The reserve is a balancing figure by construction, and the tests pin it
  against the theoretically-expected difference rather than trusting the
  construction.
- `into_trial_balance` — the translated books as a `TrialBalance`, feeding
  `consolidate` directly. The full multi-currency group close is now
  translate → consolidate, with the intercompany elimination still cancelling
  across the exchange-rate boundary.
- Multi-currency books, already-group-currency books, and any missing class
  rate (named per class) are refused.

## [0.1.0] - 2026-10-24

Initial release: same-currency consolidation with surfaced intercompany
disagreements.
