// Wire<T> — the on-the-wire shape of a generated contract type.
//
// The Rust contract maps i64/u64 to `bigint`, but JSON has no bigint: request
// bodies we build are plain numbers and `JSON.parse` yields `number` too. Rather
// than paper over the mismatch with `as unknown as T` (which erases the whole
// contract — a renamed or newly-required field would go unnoticed), `Wire<T>`
// rewrites every `bigint` field to `number | bigint` recursively. The result still
// typechecks against the contract structurally, so the compiler keeps catching a
// renamed/added/removed field while allowing a number where a bigint is declared.
//
// Precision bound: JSON numbers are IEEE-754 doubles, exact only up to 2^53 − 1
// (Number.MAX_SAFE_INTEGER). Ids beyond that would lose precision on the wire; the
// backend keeps ids within range for Phase 1a. Display always goes through
// `String(x)`, which is exact for both number and bigint.

type WireValue<V> = V extends bigint
  ? number | bigint
  : V extends Array<infer E>
    ? Array<WireValue<E>>
    : V extends object
      ? Wire<V>
      : V;

export type Wire<T> = { [K in keyof T]: WireValue<T[K]> };
