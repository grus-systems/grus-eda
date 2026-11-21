# xdomain-deferrals — Steps

Ordered. Steps 0–1 produce the measurement; step 2 is the known-divergent
fix; steps 3–4 are per-item branches; step 5 lands docs + version.
Suggested commit split:

- **(a)** fixture SDCs + `probe.sh` + committed `probe_*.txt` artifacts +
  the `docs/results.md` probe-outcome entry.
- **(b)** item-3 implementation + pins + `docs/multi-clock.md` §2/§1 edit
  + `docs/results.md` implementation note + version bump (functional
  change).
- **(c)** conditional item-2 implementation (or pin), same shape.
- **(d)** pins + doc updates for whatever stays deferred (items 1/4
  most likely) — fold into (c) if it lands empty.

## Step 0 — fixture + probe.sh

1. Write the six new SDCs per `fixture.md` (`t2_u1`, `t2_u2`, `t2_lcm`,
   `t2_lcm2` on `two_clock.def`; `t2_xlat`, `t2_xlat2` on
   `two_clock_latch.def`).
2. `probe.sh`: `run_report` timeout arg, `XDEFER` list, latch-block loop.
3. Colima/Docker route per AGENTS.md §2; run `bash
   tests/designs/sta_clocks/probe.sh`. Rerun it and confirm `git status`
   on the whole `sta_clocks/` dir shows only the new files — existing
   `probe_*.txt`/`*.reference.json` byte-stable (§7.1).

## Step 1 — read artifacts, record verdicts

Per item, from the committed `probe_t2_*.txt`:

- item 1: where do `u_fy/D` & co. land — timed table, `-unconstrained`,
  or error? Does `t2_u2`'s `-to` check time the unclocked-launch
  endpoint?
- item 2: `u_ln/D` timed? `time borrowed` lines? which clock edges?
- item 4: table / warning / error / timeout — and the edge times if a
  table appears.
- our-side: `grus-eda sta` (`--json`, `--strict`) on every new SDC;
  record current behaviour (item 2 defers today; item 1's endpoints
  no-arrival-skip; item 4 `grid_lcm` verdict + window trip count).

Write the `docs/results.md` probe entry (`docs.md` §1) and commit (a).

## Step 2 — item 3 (unpairable hold — implement)

Per `implement.md`: capture-driven `hold_edge_pair`; add the
`cross_domain_endpoints == 0` per-scenario assert in
`test_sta_clocks_match_opensta`.

Gate:

- `cargo test -p grus-core` — `t2_waverot.u_fy/D` now matches
  (`min_arr 2.287085`, `min_req 1.862415`); every golden blob
  byte-identical (no probe rerun needed — the reference numbers were
  already committed).
- Fail-proof: revert the boundary inclusion → `u_fy/D` defers → new
  assert red → restore.
- `docs/multi-clock.md`: §2 drops the item (one line in the deferral
  list), §1.7/§1.2 gains the capture-driven hold-pair rule with the
  `t2_waverot` citation.

## Step 3 — item 2 (latch foreign launch — branch)

Per `implement.md`: implement if OpenSTA borrows or checks hard-edged
(`t2_xlat*`/`t2_xlat2` move to the latch `run_sta`/`run_report` pair, fold
into `two_clock_latch.reference.json`, STRICT-compare); pin the deferral
if OpenSTA drops. Fail-proof whatever lands.

## Step 4 — items 1 + 4 (branches, most likely pins)

Per `implement.md`: pins for kept deferrals (item-1 one-clock skip test,
item-4 `grid_lcm` verdict test + `launch_window` trip bound); implement
only if a probe produced a number. Fail-proof the item-3-adjacent assert
if item 4 lands a bound.

## Step 5 — docs + version

Per `docs.md`: `multi-clock.md` §2 resolution lines, `results.md`
implementation entry, `tests/designs/README.md` scenario count,
`docs/roadmap.md` pointer line. `Cargo.toml` `0.2.11` → `0.2.12` +
`cargo check` for `Cargo.lock`, in the same commit as the functional
change.

Final gate (workflow.toml verify): `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` green; `probe.sh`
regen byte-clean.
