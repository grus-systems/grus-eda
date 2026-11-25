# drt-converge-stats — Steps

Observability ticket; no routing-behavior change. `e6-drt-e2e` has
landed (v0.12.7/v0.12.9) — the `maze.rs` line numbers below are
the current state. Keep the diff to the listed touches.

## 1. `maze.rs` — model + collection + digest

- Add `IterStat` above `RouteDb` (~194) and `iter_stats:
  Vec<IterStat>` on `RouteDb` after `iterations` (~205) —
  `model.md` for comments/semantics.
- In `route_nets` (~3727-3814): declare `let mut iter_stats =
  Vec::new();` beside `unrouted`; per loop body — `let attempted
  = bad.len() as u32;` after `iterations = it;`, `let mut routed
  = 0u32;` beside `prov`/`failed`, `routed += 1;` in the
  `Routed` arm, and `iter_stats.push(IterStat { attempted,
  routed, prov_collisions: prov.len() as u32 });` **after** the
  per-net `for` closes and **before** `if hard { … break; }`.
- Store `iter_stats` in the `RouteDb` literal (~3856).
- `route_digest` (~4026): append the len + per-entry triples
  after `put(routed.iterations)` (`digest.md`).

## 2. `drt.rs` — re-export

Add `IterStat` to the `pub use maze::{…}` list (~20-23).

## 3. `main.rs` — census lines

After the `guided=…` `println!` (~2083), before the
`unrouted_kinds` computation: two `println!`s emitting
`iter_progress=<attempted,…>` and `iter_prov=<prov_collisions,…>`
— `route_db.iter_stats` comma-joined (`output.md` grammar).
Match the block's fully-qualified style.

## 4. `tests/drt.rs`

- `maze_census`: print `{name}: iter_progress=… iter_prov=…`
  after the census line; assert the `model.md` invariants.
- New `test_route_digest_covers_iter_stats` (`digest.md`).
- Extend the `maze::{…}` import with `IterStat`.

## 5. `check_drt_ours.sh` — keep-list

Add `or s.startswith("iter_progress=") or
s.startswith("iter_prov=")` to the step-E `cli_lines` keep-list
(~934). No regex, schema, or golden change (`output.md` §1).

## 6. Verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p grus-eda
```

## 7. Evidence legs (each recorded in `docs/results.md`)

- **gcd census** — `cargo test --release -p grus-core --test drt
  test_maze_census -- --nocapture`: record `iter_progress` /
  `iter_prov` series and the new `digest=` value (the digest
  changes by construction — `digest.md`; record it, do not pin
  it anywhere).
- **gcd emit run** — release `grus-eda drt` on
  `gcd.placed.def` + `gcd.guide` into a scratch `--out-def`
  (~15 s route): capture the two new census lines verbatim into
  the results entry; confirm `iter_progress` entry count ==
  `iterations=`.
- **Fail-proof** (E0 rule 7) — the
  `test_route_digest_covers_iter_stats` gate is itself the proof
  the stats reach the digest; additionally break the loop push
  on a scratch edit and confirm `maze_census`'s
  `len() == iterations` assert goes red, then restore.
- **picorv32 profile** — `cargo test --release -p grus-core
  --test drt test_maze_census_picorv32 -- --ignored --nocapture`
  — the ~2 h ignored leg; run detached, record its
  `iter_progress`/`iter_prov` series in the results entry. This
  is the acceptance-required "picorv32 iteration profile" and
  the run that actually exercises a non-trivial tail (2,016
  unrouted at the cap on the last record).
- **Consumers** — `recount_unrouted.py gcd` still exits 0;
  eyeball that a `cli_lines` regen would carry the lines (no
  regen this ticket).

## 8. Docs + version + commit

- `docs/e6-drt.md` §7/§8 sentences (`output.md` §4).
- `docs/results.md`: dated entry — the two series for
  gcd/picorv32, new gcd digest, keep-list note, what ran.
- `docs/status.md`: one-line update.
- `Cargo.toml`: `[workspace.package] version` `0.12.10 →
  0.12.11` (feature → patch on 0.x), same commit.
- Commit: `feat: Per-iteration convergence stats in drt census`
  (Conventional, English, agent trailer).

## Contingencies

- **picorv32 leg impractical in-session** — run it detached
  before commit if feasible; otherwise land the code with the
  gcd profile recorded and the picorv32 arm named as pending in
  `results.md` (same convention as the deferred picorv32 emit
  legs), and flag that the acceptance box stays open until the
  series is recorded.
- **`iter_stats` empty on an input** — only `guided=0`; the
  unconditional lines print bare keys. Acceptable; note if it
  ever fires.
- **Digest drift vs `docs/results.md` history** — historical
  digests are dated records, not expectations; the entry states
  the cause (new serialized field) explicitly.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/check_drt_ours.sh`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
