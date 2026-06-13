# ADR 0003 — Economic parameters are versioned config, not code

Status: accepted · Date: 2026-06-13

## Context

The owner stated the tokenomics (token secondary layer, monetization, the knobs)
will likely change. The Dossier itself frames the company skim, content fee, LP
seed E₀, and calibration constants (λ, κ, γ, α) as tunable knobs with proposed
defaults — "nothing economic is locked."

## Decision

All economic constants live in one struct, `EconParams` (`crates/econ-params`),
carrying a `version` field, loadable from TOML. No economic constant is hardcoded
anywhere else in the codebase.

## Rationale

A tokenomics change becomes: edit the config, bump `version`, re-run the
invariant tests, (for 1b) re-audit. It is never a code rewrite. The exact
parameter set an epoch settled under can be snapshotted (the struct serializes
back to TOML), which the audit trail and reproducibility need.

## Consequences

- Defaults in `EconParams::default()` mirror Dossier §10; treat them as proposals.
- The settlement record should store the params version (and ideally the
  serialized params) used for each epoch, so a replay is bit-identical.
- Calibration constants are kept private in production (Gamma remark 10.3);
  config files with real values are gitignored / secrets-managed, not committed.
