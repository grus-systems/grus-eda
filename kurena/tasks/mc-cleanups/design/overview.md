# mc-cleanups — Overview (revision 3)

Cleanup bundle for the low-severity findings left over from the
multi-clock sequence plus the follow-up review memos.

**Status**: revision 1 landed as `e6bd1a7` (v0.2.7), revision 2 as
`25f9700` (v0.2.8) — the entry-list `delay_at` fold, the hardened
endpoint collector, the five promoted scenarios and the doc pass are
all in (see the 2026-11-20 `docs/results.md` entry and its revision
subsection). The second human review returned **one medium finding and
two optional lows**; this revision is the follow-up.

Version: functional change → `0.2.8 → 0.2.9` in the same commit
(`[workspace.package]`, `Cargo.lock` refreshed).

## Review-memo inventory (the remaining work)

| # | Item | Kind | Where |
|---|------|------|-------|
| M1 | **Same-anchor same-sense retention in `delay_at`** — two `-max -clock cka` rules accumulating into a worst-fold — is *inferred* from the different-clock prune measurement (`t2_clkcor`/`t2_clkcor2` only measured different anchors and `-add_delay`), and unit tests now assert the inferred fold. Per the E0 rules: **probe the same-clock duplicate-rule case in the container and implement the measured result** (`t2_clkdup`, probes.md P5). Contingency, only if the container genuinely cannot run: keep the fold and mark it explicitly *inferred/unmeasured* in the code comment, the asserting test comments, `docs/multi-clock.md` and `docs/results.md`. | probe + code | `t2_clkdup.sdc` (new), `sdc.rs` `delay_at` + tests, `analysis.rs` test count |
| L1 | `tests/designs/README.md` says "Since `mc-selectors` (v0.2.5) all 45 golden scenarios evaluate" — misattributed: the count was 40 at v0.2.5 and became 45 at v0.2.8; with `t2_clkdup` it becomes **46** at v0.2.9. Reattribute the count to the version that introduced it. | docs | `tests/designs/README.md` |
| L2 | `set_clock_groups` unknown-name posture is unrecorded in `docs/`: `-group [get_clocks no_such]` filters to an empty group (the `clock_rows`/`t2_selunk` posture), a bare unknown name bails `unknown clock` (the `resolve_clock` posture) — neither measured on this command; say so. | docs | `docs/multi-clock.md` |

## What the medium item is about

`sdc.rs:384–397` — `delay_at` folds a port's `DelayRule`s into a
`Vec<DelayEntry>`: a plain (non-`-add_delay`) rule `retain`s only
entries with `e.clock == r.clock`, then appends its own. The
different-anchor half is measured (`t2_clkcor` both-corners loss,
`t2_clkcor2` single-anchor survival, `yb` merged anchors). The
**same-anchor** half is not: `retain` keeps same-clock entries
unconditionally, so two same-clock `-max` rules accumulate and the
worst check wins. OpenSTA could instead overwrite a per-(clock,
corner) slot — that hypothesis is equally consistent with every
committed measurement (`t2_int`'s `-min`/`-max` pair survives under
both, since different senses never conflict). Only a same-clock
same-sense duplicate discriminates — that is what `t2_clkdup` measures.

## Probe plan (one container run)

`tests/designs/sta_clocks/probe.sh`, same container
(`ghcr.io/librelane/librelane:3.0.16`): one new scenario `t2_clkdup`
joins `SCENARIOS` — same-clock same-sense duplicates on `d_in`/`ya`
plus a `Both`-then-`-max` same-clock sequence on `yb` that separates
per-corner-slot overwrite from whole-entry overlap removal. The
collector needs no further changes (union + `null` fields landed in
the revision-2 hardening). See `probes.md` P5 for contents and the
discriminator table.

## Non-goals

- No new SDC commands or selector namespaces; no change to the
  entry-list model itself — only the same-anchor prune key is pinned.
- `-add_delay` semantics unchanged (gcd widening already measured).
- Unanchored (`-clock`-less) duplicates are not probed: unanchored
  entries produce no check either way, so only diagnostics folds could
  differ — not endpoint-visible.
- L1/L2 are optional lows — do them in the same commit if cheap.

## Verification gates

1. `cargo test --workspace` — `evaluated` becomes 46
   (45 `two_clock` + `t2_latch`); updated unit asserts.
2. `cargo clippy --workspace --all-targets -- -D warnings` — clean.
3. `git diff tests/designs/`: `two_clock.reference.json` append-only
   (+1 block); every pre-existing `probe_t2_*.txt` and golden block
   byte-identical — drift means a committed scenario exercises the
   changed fold; stop and diagnose.
4. Fail-proof: swap the prune predicate to the unmeasured alternative,
   confirm `t2_clkdup` red on the discriminating fields, restore.
5. `docs/results.md` dated continuation: verbatim probe lines,
   discriminator read-off, rule implemented (or the inferred-marking
   record if the contingency ran).

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs` (test count only)
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_clkdup.sdc` (new)
- `tests/designs/sta_clocks/two_clock.reference.json`
- `tests/designs/sta_clocks/probe_t2_clkdup.txt` (new)
- `docs/results.md`
- `docs/multi-clock.md`
- `tests/designs/README.md`
- `Cargo.toml`
- `Cargo.lock`
