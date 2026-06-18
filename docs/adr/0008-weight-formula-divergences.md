# ADR 0008 — Known divergences of the implemented weight formula from the dossier

Status: accepted (documented, not yet reconciled) · Date: 2026-06-17

## Context

A cold review compared `gem_engine::weight` (backend/crates/gem-engine/src/lib.rs)
to the dossier's social-weight definition (§II.3 / App. C) and found the code
diverges from the spec in two ways that were previously **undocumented**. The math
is faithful on the big structure (log-space sum, hard gate `v_i`, concave `β`,
PageRank `NS`, unique factor `U`) but differs on two terms. Recording it so it is a
known, deliberate state — not a silent gap a reviewer "catches".

Dossier §II.3:
```
log w_i = log v_i + log β(B_i) + log(Σ ω·e^−λτ) + log NS_i + log(1+U_i) + log T_i + γ·log(1+a_i)
```
Code (`weight`):
```
log w_i = [v_i gate] + log β + log(volume) + log NS_i + log(1+U_i) + γ·log(1+a_i)·NS_i
```

## The divergences

1. **Audience term is coupled to the node score.** The code uses
   `γ·log(1+a_i)·NS_i` (audience scaled by PageRank) where the dossier has a
   standalone additive `γ·log(1+a_i)`. Effect: audience matters more for
   well-connected users than the spec intends. Whether this coupling is desired
   ("reach counts more for influential accounts") or an error is a calibration
   question for the dossier author — it is NOT obviously a bug, so the code is left
   as-is and flagged, not silently rewritten.
2. **The `T_i` term is omitted.** The dossier's `log T_i` factor has no defined
   input in the current model (no source for `T_i` exists in `UserInputs`), so it
   is simply absent rather than stubbed.

## Decision

Document both as **known Phase-1a divergences**, to be reconciled during economic
calibration (before real money flows in Phase 1b), not unilaterally "fixed" now:
changing the weight math changes payouts and is an economic/dossier decision, not
an engineering one. The relevant knobs (`gamma_audience`, `time_decay_lambda`, the
`kappa_*`) are already versioned config (ADR 0003), so a reconciliation is a param
+ formula change in one isolated place, re-validated by the conservation proptest.

Related fidelity note: **the time-decay `e^−λτ` is currently intra-epoch only.**
Settlement reads one epoch's edges (one day) and measures τ to that epoch's close,
so τ ∈ [0,1] day and the λ default's ~7-day half-life cannot actually bind until
settlement considers multi-day windows. The decay is wired and correct for what it
spans (newer-in-the-day counts slightly more); its full effect is deferred with the
rest of the multi-day economics.

## Consequences

- These do not affect Phase-1a points (no real value moves), but MUST be resolved
  before Phase 1b. Tracked here + in CLAUDE.md so they aren't forgotten.
- No behaviour change in this ADR; it is documentation of existing code. See
  [[0003-economic-params-are-config]] and [[0007-adopt-v6-economic-spine]].
