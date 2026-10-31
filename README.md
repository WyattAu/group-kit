# group-kit

Multi-entity consolidation: several books, one group, and the intercompany
positions between them.

## The rule that shapes everything

**Eliminations must cancel, and anything that does not cancel is surfaced,
never guessed.** An intercompany position has two sides recorded in two sets
of books. When both agree, the elimination removes both and the consolidated
intercompany balance is zero. When they disagree in amount, or only one side
was recorded, the consolidation *refuses to eliminate* and names the
disagreement; eliminating the lender's figure unilaterally would silently
decide whose books are right, and a wrong elimination moves group profit while
the elimination schedule still looks tidy.

This is the reconciliation lesson applied to group accounts: ambiguity is an
output, not something a heuristic resolves.

## What consolidated means here

Rows are summed per `(account, currency)` across entities, then eliminations
are applied to the named intercompany accounts. The invariants a reader
checks first hold by construction: every eliminated position nets the
intercompany accounts to zero, and consolidated debits equal consolidated
credits, because an elimination removes equal amounts from both sides.

## Scope, stated rather than implied

**Same-currency consolidation.** Entities keeping books in other currencies
restate into the group currency first (closing rates for monetary items,
average for P&L); translation is a separate upstream step with its own rules
and its own failures, and folding it into elimination would entangle them. A
currency mix is refused, not translated.

**No non-controlling interest in v1.** Wholly-owned groups only; an NCI
allocation changes which equity subtotal the result lands in and needs the
ownership schedule behind it.

## Licence

MIT OR Apache-2.0.
