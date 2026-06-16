# ADR 0005 — The bot gate is operator-set, never self-asserted

Status: accepted · Date: 2026-06-16

## Context

The bot gate `v_i` is the hard veto in the weight function: an unverified user
earns zero gems (Dossier §4.4). The 2026-06-16 audit found it was effectively
unwired and exploitable:

- It defaulted to `false`, registration hardcoded `false`, and nothing ever set
  it `true` — so a freshly deployed economy paid zero to every real user.
- The only way to set it was the legacy, UNAUTHENTICATED `POST /users`, which
  accepted a client-supplied `bot_gate_v` — letting anyone self-verify into the
  gem-earning set without limit, inverting the gate it was meant to be.

## Decision

- Remove the public `POST /users` route. Account creation is `/auth/register`,
  which always creates an UNVERIFIED user.
- The bot gate is mutable only via an operator-only endpoint,
  `PUT /users/:id/verification`, guarded by the `AdminUser` (operator) extractor.
  `bot_gate_v` is never deserialized from a client on any creation path.

## Rationale

Verification eligibility is a privileged, abuse-sensitive decision; it must be a
server-side action by a trusted operator, not a self-assertion. For Phase 1a a
manual operator action is sufficient (the migration always intended "manual
early"); the gain here is that a real mechanism now exists, is authorized, and is
tested (401/403/200), and the self-verify hole is closed. See
[[0004-ledger-journal-and-atomic-settlement]] for the related admin-auth work
(operator-only settlement).

## Consequences

- A fresh deployment still pays nobody until an operator verifies users — by
  design, fail-closed. A heuristic/automated verification policy (e.g. email
  confirmation + minimum account age) is a deliberate later step, not wired yet.
- Tests seed verified users via the repository directly (trusted server code),
  which remains the path internal callers and fixtures use.
