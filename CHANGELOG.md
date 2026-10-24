# Changelog

All notable changes to this project are documented here. Format: [Keep a
Changelog](https://keepachangelog.com/) — versions follow [semver](https://semver.org).

## [Unreleased]

## [0.1.2] - 2026-10-24

### Changed

- Tracks `double-entry 0.1.1` (docs-only upstream release, identical API). Pin move keeps one version in a consumer graph that binds 0.1.1.


## [0.1.1] - 2026-10-24

### Changed

- Tracks  (docs-only upstream release, identical API). Pin
  move keeps one version in a consumer graph that binds 0.1.1.


## [0.1.0] - 2026-10-24

Initial release.

### Added

- `consolidate` — entity trial balances plus intercompany positions to a
  consolidated set of group rows.
- Pairwise intercompany matching: a mirror with a disagreeing amount is an
  `IntercompanyMismatch` naming both figures; a missing mirror is
  `OneSidedIntercompany`. Neither is ever eliminated by guesswork.
- Unbalanced entity books and foreign-currency entities are refused before
  any elimination — two cancelling imbalances must not launder into a
  balanced group.
