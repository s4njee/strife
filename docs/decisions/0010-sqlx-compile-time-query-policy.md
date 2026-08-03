# ADR 0010: SQLx Compile-Time Query Policy

## Status

Accepted.

## Context

Strife originally had one SQLx macro query and an offline cache with one entry.
The rest of the API and database layer used runtime `query`, `query_as`, and
`query_scalar` calls, so `SQLX_OFFLINE=true` proved only that Rust compiled; it
did not validate the SQL that had caused production decode failures.

The original Epic 8 inventory counted 88 runtime calls. The email, OCR, backfill,
and repair work expanded the current inventory to more than 200. A trial bulk
conversion showed why a single mechanical rewrite is unsafe: SQLx cannot infer
Strife's Rust mappings for PostgreSQL enums, `tsvector`, shared `FromRow` records,
synthetic response fields, dynamic statements, or the nullability of many CTE and
aggregate projections. Using the unchecked macros would create cache entries but
would not catch the output type errors this story exists to prevent.

## Decision

1. New statically expressed API queries use `query!`, `query_as!`, or
   `query_scalar!` and explicit SQLx nullability overrides where PostgreSQL cannot
   prove a non-null result.
2. The storage aggregate queries that motivated this work are macro checked.
   Their `BIGINT` output is pinned by the offline metadata and cannot silently
   regress to a numeric type decoded as `i64` only at runtime.
3. Runtime-checked calls are permitted only when present in the generated
   [runtime query inventory](../development/sqlx-runtime-queries.md). The
   inventory identifies every call and its reason class; CI regenerates it and
   fails on drift.
4. We do not use `query_unchecked!`, `query_as_unchecked!`, or
   `query_scalar_unchecked!`. They would hide precisely the output mismatch the
   policy is meant to catch.
5. Each database domain moved out of `crates/db/src/lib.rs` by Story 12.1 must
   convert its inventoried queries while it owns the required explicit enum and
   projection annotations. This keeps those type-affecting SQL rewrites separate
   from the pure module move.
6. `.sqlx/` is generated only from a fully migrated database. `make
   sqlx-prepare` refreshes it; `make sqlx-check` and CI reject stale metadata.
7. CI also compiles an intentionally wrong SQLx scalar assignment and requires
   compilation to fail, proving that the online macro/type guard is active.

## Consequences

The highest-risk API and aggregate queries now fail at compile time and work in
offline builds. Runtime exceptions remain visible and cannot grow silently. The
database module retains its tested `FromRow` behavior until each domain can add
explicit projections without combining a type-contract rewrite with the large
module split.

The exception inventory is intentionally strict and somewhat noisy: moving a
runtime query changes its recorded line and requires regeneration. That review
friction is the enforcement mechanism, not incidental documentation.
