# drt-converge-stats — Steps (r3, memo corrections)

HEAD is 08141b1 — the full feature surface is landed and green.
This round is three small corrections per the review memo. No
routing-behavior change, no CLI output change, no digest change.

## 1. `tests/drt.rs` — weaken the monotonicity assert

Three hunks inside `maze_census`:

a. **Informational print** — after the
   `{name}: iter_progress=… iter_prov=…` println (:1641-1653),
   add:

   ```rust
   println!(
       "{name}: iter_progress_monotonic={}",
       r.iter_stats
           .windows(2)
           .all(|w| w[0].attempted >= w[1].attempted)
   );
   ```

   Empty/single-entry `iter_stats` → `all()` on an empty
   iterator → `true`; acceptable (vacuous), no special-casing.

b. **Remove** the `for w in r.iter_stats.windows(2)` assert
   block (:1708-1713) — `w[0].attempted >= w[1].attempted` is
   not a structural invariant (`model.md`).

c. **Fix the comment** (:1680-1685) — drop "`attempted` is
   non-increasing (the re-queue is a subset)". Replace with the
   mechanism-accurate note: `bad[i+1] = failed ∪ both parties of
   every provisional collision`, and a collision's foreign party
   may be a kept net outside `bad[i]`, so `attempted` can rise —
   observed non-increasing, reported not asserted.

   `maze.rs` needs no edit: its comments were audited — the
   `// bad = unrouted ∪ both parties of every provisional
   collision.` note (:3818) states the construction verbatim and
   never claims `bad[i+1] ⊆ bad[i]`; the `IterStat` doc comments
   (:194-212) make no monotonicity claim either.

## 2. `docs/results.md` — dated-entry corrections

In the `drt-converge-stats` entry (:10152-10249):

- Header `### 2026-11-25` → `### 2026-11-26` — the picorv32
  splice (the entry's completion) ran 2026-11-26 and already
  says so inline.
- Annotate the gcd legs inline "run 2026-11-25" (census +
  emit-leg paragraphs) — once the header carries the splice
  day, the gcd legs' own date would otherwise be lost (E0
  lesson 9: say what you ran, dated). The picorv32 paragraph's
  inline `run 2026-11-26` stays as the model.
- Fail-proof paragraph (:10204-10208): drop "`attempted`
  non-increasing," from the asserted-invariant list; append that
  monotonicity is reported informationally
  (`iter_progress_monotonic=true` on the gcd leg) — empirical,
  not gated.
- Picorv32 paragraph (:10237-10241): same — "non-increasing"
  leaves the asserted list; the leg reported
  `iter_progress_monotonic=true`. Add the one-line reason a
  future leg could print `false` (kept-net collision parties
  re-queue from outside `bad[i]`).
- "What ran" (:10243+): extend with this round's verify run.

Keep the verbatim census blocks and both digest numbers
byte-identical — they are run records.

## 3. `docs/e6-drt.md` — semantics clause + punctuation

- §7 (:560-562): after "`bad` = unrouted nets ∪ both parties of
  every provisional collision", add one clause — a collision's
  foreign party may be a kept net (committed in an earlier
  iteration, marks still in `occ`), so `bad[i+1]` need not ⊆
  `bad[i]` and `iter_progress` is not guaranteed non-increasing.
- §8 (:709-713): fix the `—,` join — rewrite "…from
  `RouteDb.iter_stats` — the last `iter_prov` entry is always 0,
  the hard iteration reporting no provisionals —, one `unrouted
  <name>` line…" e.g. by switching the aside to parentheses:
  "…series from `RouteDb.iter_stats` (the last `iter_prov`
  entry is always 0 — the hard iteration reports no
  provisionals), one `unrouted <name>` line…".

## 4. Verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Then `cargo test --release -p grus-core --test drt
test_maze_census -- --nocapture` and confirm the gcd census
prints `iter_progress_monotonic=true` (the informational line is
the only new output; the iter_progress series must still be
`269,94,87,81,81,81,81,81`). No picorv32 re-run needed — the
recorded series stands; the assert removal only loosens the leg.

## 5. Commit

One commit covering §1–§3 (test + docs correction is one logical
fix):

```
fix: Report iter_progress monotonicity instead of asserting it

- maze_census: drop the windows(2) non-increasing assert —
  bad[i+1] can gain kept nets re-queued as provisional-collision
  parties, so attempted non-increase is empirical, not
  structural; add an informational iter_progress_monotonic=
  print and fix the stale subset comment
- docs/e6-drt.md: name the kept-net re-queue case in §7; fix the
  §8 em-dash join
- docs/results.md: header date to the picorv32 splice day;
  invariant lists mark monotonicity observed-only

<agent trailer>
```

No `Cargo.toml` bump — a test/doc correction with no shipped
behavior change (the 0.12.11 feature bump is already landed).

## Contingencies

- **`iter_progress_monotonic=false` on a future leg** — not a
  failure by design; record it in `docs/results.md` as the first
  observed kept-net re-queue if it ever appears.
- **Any of the remaining `iter_stats` asserts goes red** — real
  defect, treat as such; do not weaken further without naming
  the mechanism.

## Files

- `crates/grus-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
