# OpenSTA probe plan — measured two-clock rules

Each section states the question, the scenarios that answer it, the
hypotheses the numbers discriminate, and what to write into
`docs/results.md`. All numbers must come from `probe.sh` inside
`ghcr.io/librelane/librelane:3.0.16` — never derived by formula
(AGENTS.md §7 rule 2). Where a `probe_*.txt` full-format report is
the evidence, cite the file and line in results.md.

## Q1 — are cross-domain paths timed, and how are launch/capture pairs formed?

Scenarios: `t2_int` (2.0 / 1.0), `t2_frac` (2.0 / 1.4).

Expectation (to be confirmed, not assumed): OpenSTA times
cross-domain paths by default — clocks are "related" unless
`set_clock_groups`/false paths say otherwise. Evidence:

- `t2_int` endpoint table: `u_fx/D` and `u_fy/D` present with
  ordinary-looking arr/req, not absent, not in `-unconstrained`.
- `probe_t2_int.txt`: full-format reports name the launch and capture
  clock per path (`clock cka (rise edge)` / `clock ckb (rise edge)`).

For `t2_int`, derive the pairing from edge times:

- A->B (`u_fa0 -> u_fx`): launches on cka edges {0, 2, 4, ...},
  captures on ckb edges {0, 1, 2, 3, ...}. Setup capture should be the
  first ckb edge strictly after the launch edge — record which edge
  pair the report shows.
- B->A (`u_fb0 -> u_fy`): launches {0, 1, 2, ...}, captures
  {0, 2, 4, ...} — record.
- Hold pairing (min table): which same-time/edge-index pairs OpenSTA
  picks cross-domain — record both directions.

## Q2 — non-integer periods: capture-edge search horizon

Scenario: `t2_frac` (2.0 / 1.4).

Hypotheses for the setup capture rule on a cross path:

- **H-first**: capture is always the first capture-domain edge
  strictly after the launch edge (per-launch, no global worst case).
- **H-window**: OpenSTA enumerates launch/capture edge pairs over a
  common window (~LCM of the periods) and reports the most
  restrictive pair — for setup the pair minimising
  `(capture_time - launch_time) > 0`; for hold the pair minimising
  the gap in the opposite direction.

Discriminator: with cka=2.0/ckb=1.4, edge grids are
A={0,2,4,6,8,10,12,14}, B={0,1.4,2.8,4.2,5.6,7,8.4,9.8,11.2,12.6,14}.
H-first on a launch at 0 predicts capture at 1.4 everywhere. H-window
predicts the *smallest positive* capture−launch gap across the
window: launch 8.0 -> capture 8.4 (Δ0.4), launch 12 -> 12.6 (Δ0.6),
launch 6 -> 7.0 (Δ1.0) — the min is 0.4 ns, which would tighten the
required time versus H-first's 1.4 ns. The `probe_t2_frac.txt` clock
edge lines and the required times settle which model OpenSTA uses;
also record whether the reported path's launch edge is the 0-edge or
a later one (H-window picks a later pair).

Same for the B->A direction (launch grid 1.4, capture grid 2.0):
launch 2.8 -> capture 4.0 (Δ1.2) vs launch 4.2 -> 6.0 (Δ1.8) etc.

And hold (min): record the coincident/nearest-edge pair OpenSTA
selects — this is the rule a multi-clock hold implementation needs.

## Q3 — per-clock uncertainty

Scenarios: `t2_unc`, `t2_uncx`.

Questions, answered per endpoint class:

- same-domain A path (`u_fa0 -> u_fa1`): does req shift by cka's
  `-setup 0.05`? same-domain B (`u_fb0 -> u_fb1`) by ckb's 0.20?
- cross A->B (`u_fx/D`): is the applied uncertainty the capture
  clock's, the launch clock's, the max, or the sum? Compute each
  candidate from `t2_frac`'s req and subtract — they differ enough
  (0.05 / 0.20 / 0.25) to discriminate at 1 ps.
- cross B->A (`u_fy/D`): symmetric check — reveals whether the rule
  is "capture side" vs "sum" (sum gives 0.25 both ways; capture-side
  gives 0.05 on B->A, 0.20 on A->B).
- `t2_uncx`: does `-from cka -to ckb -setup 0.30` override exactly the
  A->B cross paths and leave B->A and same-domain untouched? And does
  it interact with the per-clock values (replace vs add)?
- hold side: `t2_unc`'s `-hold 0.10 [get_clocks cka]` on `u_fa1`/`u_fy`
  min reqs.

## Q4 — per-clock latency

Scenario: `t2_lat` (network 0.6 on cka, 0.2 on ckb, `-source` 0.1 on
cka; ideal clocks).

For each endpoint decompose: `req` should carry the *capture*
domain's latency and `arr` the *launch* domain's (plus `-source`
added where propagation is off, per the existing measured rule). The
cross endpoints discriminate launch-vs-capture attribution because
0.6 and 0.2 differ. Record per-endpoint which latencies composed the
number.

## Q5 — delay checks (`set_max_delay`/`set_min_delay`) on second-domain endpoints

Scenario: `t2_md` (cell endpoints), `t2_mdo` (output port).

The §5 question: for `set_min_delay 0.5 -from u_fb0 -to u_fy`
(capture domain cka) and `set_max_delay 1.0 -from u_fa0 -to u_fx`
(capture domain ckb), does the delay-check required time anchor at
the **endpoint's own domain clock** (the existing measured rule
`req = capture_clk_arrival(edge0/edge) - unc - t_late + N` /
`req = req_hold_base + N`, specialised per domain)?

Hypotheses:

- **H-capture-domain**: req built from the capture clock's edges
  (ckb for `u_fx`, cka for `u_fy`) — the per-domain generalisation of
  today's rule.
- **H-launch-domain**: req anchored to the launch clock — would read
  like today's skewed-launch `pm2_min2` case crossing domains.
- **H-other**: some other anchor (e.g. delay-edge of whichever clock
  arrives first).

`t2_md`'s req values discriminate: cka edge0 vs ckb edge0 differ
(period 2.0 vs 1.0, different latencies if set). Also record the
arrival side — does the delay check keep the covered source's
ordinary arrival (per the `pm2_both` rule) now that the launch is a
different domain's edge?

`t2_mdo` checks the output-port form: `req = N - out_delay - unc`
with the `-clock ckb` domain — record whether anything
domain-dependent enters at all.

## Q6 — propagation and clock groups

- `t2_prop`: `set_propagated_clock [all_clocks]` — both networks
  propagate; cross paths see skewed launch/capture on their own nets.
  Record the capture arrival per endpoint.
- `t2_prop1`: propagate only cka — OpenSTA applies propagation per
  clock: cka pins propagated, ckb pins ideal. Confirms per-domain
  `propagated_clock` must be a per-clock flag, not one bool.
- `t2_grp`: `set_clock_groups -asynchronous` — expected: `u_fx/D` and
  `u_fy/D` leave the timed set (`-unconstrained` or absent), same-domain
  paths untouched. Record exactly which endpoints disappear.
- `t2_mcp` (stretch): MCP multipliers on a cross path — which
  domain's period scales `-setup 2` (expected: capture side for
  setup, and record what `-hold 1` does).

## Recording format

`docs/results.md` entry (dated), two parts:

1. Break-point table from `breakpoints.md` §3 (command → observed
   behaviour).
2. Measured-rules table: one row per Q above — scenario file,
   discriminating numbers (edge times, req decompositions), the rule
   OpenSTA follows, and the hypothesis that won. Rules go verbatim
   into `docs/multi-clock.md`'s spec section so the implementation
   tickets cite measured behaviour, not guesses.

## Failure handling

- If a scenario's OpenSTA run errors (`read_sdc` failure is captured
  per the existing harness), record the error and drop the scenario —
  but say so in results.md.
- If req decomposition matches no hypothesis, stop and decompose
  per-transition before writing the rule; an unexplained number is a
  finding, not a table cell to fudge.
- Numbers must be reproducible: `probe.sh` re-run must regenerate
  identical artifacts — `git diff` them before committing (AGENTS.md
  §7 rule 1).
