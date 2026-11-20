# mc-cleanups — Container probe plan (revision 2)

One container run of `tests/designs/sta_clocks/probe.sh` inside
`ghcr.io/librelane/librelane:3.0.16`. The revision adds three scenarios
(`t2_eclk2`, `t2_clkcor`, `t2_clkcor2`), extends one (`t2_eclk`), and
promotes two (`t2_waverot`, `t2_wavez`) plus the three new ones into
`SCENARIOS`. Every scenario is a variant of `t2_int.sdc` — copy it
verbatim and edit the listed lines; verify pin/cell names against
`two_clock.def`.

```tcl
# t2_int.sdc base (verify against the committed file — do not reconstruct):
create_clock -name cka -period 2.0 [get_ports clk1]
create_clock -name ckb -period 1.0 [get_ports clk2]
set_input_delay -min 0.1 -clock cka [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -max 0.7 -clock cka [get_ports ya]
set_output_delay -min 0.1 -clock ckb [get_ports yb]
set_output_delay -max 0.7 -clock ckb [get_ports yb]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
```

## P0 — `collect_endpoints_json` hardening (before any new scenario)

Mixed-anchor scenarios produce corner-only endpoints: `u_fa0/D` stays in
`paths_max` but drops out of `paths_min` when `d_in` loses its min-side
anchor (and vice versa). The current collector builds `pins` from
`paths_max` only and reads `min_*($name)` unconditionally — an
endpoint missing from `paths_min` hits `can't read "min_arr(...)"`
(Tcl error, whole scenario dead), and a min-only endpoint silently
never enters `pins` at all.

Fix `run_sta`'s embedded Tcl:

- `lappend pins $name` in **both** passes (`paths_max` and
  `paths_min`), guarded by `[info exists max_arr($name)] == 0` so each
  pin is listed once.
- Emit `null` for unset corner fields:
  `set v [expr {[info exists max_arr($name)] ? $max_arr($name) : "null"}]`
  per field (`max_arr_ns` … `min_slew_ns`).
- `run_report` needs no change — `report_checks` prints whatever exists;
  `report_checks -unconstrained` lists what OpenSTA considers untimed.
- Tcl discipline: no `!` on stdin (E0 lesson 3) — use `== 0` / `info
  exists` forms only.

Every committed scenario currently has both corners on every endpoint,
so the hardened collector reproduces the existing golden byte-for-byte.

## P1 — `t2_clkcor` / `t2_clkcor2`: mixed `-clock` rule sequences (R1)

`t2_clkcor.sdc` — ordering "`-clock` rule first, bare rule second":

```tcl
… base, except:
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_input_delay -min 0.1 [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -max 0.7 [get_ports ya]
```

`t2_clkcor2.sdc` — reversed ordering on the same ports, plus an
`-add_delay` anchor discrimination on `yb`:

```tcl
… base, except:
set_input_delay -min 0.1 [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_output_delay -max 0.7 [get_ports ya]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -min 0.1 -clock ckb [get_ports yb]
set_output_delay -max 0.7 -clock cka [get_ports yb]
set_output_delay -max 0.4 -clock ckb -add_delay [get_ports yb]
```

**Questions:**

1. Does a `-clock`-less rule touching one corner declock the port
   entirely (our current behaviour — `delay_at` assigns `clock =
   r.clock` for every matching rule regardless of `sense`), or does the
   `-clock` rule anchor only its own corner?
2. Does rule order matter (last-rule-that-touched-the-corner vs any
   other fold)?
3. Under `-add_delay`, does the corner's anchor follow the last rule or
   the rule that produced the winning value — or does OpenSTA merge
   *both* entries' clocks?

**Discriminators** (`probe_t2_clkcor*.txt` full-format reports +
`ref_t2_clkcor*.json` endpoint blocks):

| outcome | `d_in` (max anchored to `cka`, min bare) | `ya` (min anchored to `cka`, max bare) |
|---|---|---|
| per-corner anchor | `u_fa0/D`/`u_fb0/D` in the **max** table only — arr `0.7`-class on `cka`; absent from `paths_min` (`min_*` → `null` in the ref) | `ya` in the **min** table only — req on `cka` grid; absent from `paths_max` |
| whole-port declock (ours today) | `d_in` never launches → `u_fa0/D`, `u_fb0/D` gone everywhere (`t2_noclk` shape, 12 endpoints) | `ya` gone everywhere |
| port-wide last-rule clock | min side anchored to `cka` anyway → `u_fa0/D`/`u_fb0/D` present in both tables with `0.1`-class min arr | `ya` present in both, req on `cka` both corners |

`t2_clkcor2` ordering B produces the identical table under per-corner
anchoring and a different one under port-wide last-rule — the pair of
files separates the two hypotheses without extra runs.

For `yb` (`-max 0.7 -clock cka` then `-max 0.4 -clock ckb -add_delay`,
folded max delay `0.7` either way):

| `yb` max req (edge-0 + period − delay − setup_unc) | reading |
|---|---|
| `2.0 − 0.7 − 0.2 = 1.1`, "clocked by cka" | anchor follows the **winning** value's rule (`cka`) |
| `1.0 − 0.7 − 0.2 = 0.1`, "clocked by ckb" | anchor follows the **last** rule regardless of winner |
| `min(1.1, 1.0 − 0.4 − 0.2) = 0.4` | OpenSTA merges **both** entries' anchors |

If the merged-entries reading shows up, also check the `d_in`-launched
arrs for a second clock's edge — implementation impact is larger; see
`api.md` R1 for the fallback.

Promotion: both files join `SCENARIOS`. If our implementation of the
measured rule diverges on an endpoint field, that is a bug to fix in
the same commit; demote only with a named root cause recorded.

## P2 — `t2_eclk` extension (R4, R5)

The committed `t2_eclk.sdc` ends with:

```tcl
set_clock_latency 0.5 [get_clocks {}]
set_clock_uncertainty -setup 0.2 [get_clocks {}]
set_clock_transition 0.1 [get_clocks {}]
set_propagated_clock [get_clocks {}]
set_clock_latency 0.5 [get_clocks no_such]
```

Code comments in `sdc.rs` already cite a "`t2_eclk` family" for forms
the file does not exercise. Append lines that measure them (keep the
existing five lines; put the riskier forms last — if one line aborts
`read_sdc`, split it into a `t2_eclk3.sdc` so each error-prone form is
in tail position):

```tcl
set_clock_uncertainty -setup 0.2 -from [get_clocks {}]
set_clock_uncertainty -setup 0.2 -from cka -to [get_clocks {}]
set_propagated_clock [get_clocks no_such]
unset_propagated_clock [get_clocks {}]
```

**Questions:** per form — warn (which STA code), silent no-op, or SDC
error? `set_propagated_clock [get_clocks no_such]` specifically pins the
non-clock-name drop (R5): `STA-0351` warn + continue, silent drop, or
error?

**Discriminator:** warning/error lines in `probe_t2_eclk.txt`; the
table stays the `t2_int` shape iff every form is a no-op
(`set_clock_uncertainty -from`/`-to` empties record no inter-clock rule
→ req times unchanged).

The collection-form empty group is already measured by `t2_grpe` —
`-group {} -group [get_clocks ckb]` gives the same 5-endpoint table as
`t2_grp` (the empty group contributes nothing; `{ckb}` still cuts
cross-domain). If a `-group [get_clocks {}]` collection behaves
differently than the bare `{}` form it is worth one more scenario —
add `set_clock_groups -asynchronous -group [get_clocks {}] -group
[get_clocks ckb]` to `t2_eclk` only if the probe budget allows, and
read it against the committed `t2_grpe` table.

`t2_eclk` stays in `MEASURE_ONLY`; `probe_t2_eclk.txt` regenerates and
is re-committed (it is this ticket's own artifact, not a shared
golden).

## P3 — `t2_eclk2`: `[get_clocks]` no-arg → all clocks (R2)

The rule "`[get_clocks]` with no argument returns every declared clock"
was recorded from an ad-hoc stdin check — no committed artifact backs
it (E0 rule 1 violation). Committed scenario:

```tcl
# t2_eclk2.sdc — base + appended:
set_clock_latency 0.5 [get_clocks]
set_clock_uncertainty -setup 0.15 [get_clocks]
```

**Discriminator:** if `[get_clocks]` returns all declared clocks, every
`clock … (rise edge)` line shifts +0.5 (both domains) and every req
drops a further 0.15 — decisive and endpoint-visible. If it returns an
empty/errored collection instead, the table stays `t2_int` and the
implemented `all_clock_names()` path is wrong.

Joins `SCENARIOS` (real timing semantics, now implementable and
measured): `ref_t2_eclk2.json` folds into the golden.

## P4 — `t2_waverot` / `t2_wavez` promotion (R3)

Both scenarios move from `MEASURE_ONLY` to `SCENARIOS` — they run
`run_sta` (endpoint JSON folded into the golden) **and** `run_report`.
A5 already stores the wrapped pair as measured (`[1.9, 0.1]` kept,
`[0, 0]` collapsed); promotion makes the golden pin the endpoint-level
consequences at STRICT instead of the report text alone.

Risk: `t2_wavez`'s `[0, 0]` clock is degenerate for pairing (rise =
fall = 0). `probe_t2_wavez.txt` shows OpenSTA produces a normal
`t2_int`-shaped table (all-rise-triggered fixture — the fall instant is
unobservable here). If our eval diverges anyway (e.g. `hold_edge_pair`
on a zero-width duty), the fix is in scope if small; otherwise demote
`t2_wavez` back to `MEASURE_ONLY`, keep the committed `probe_`/`ref`
evidence path, and record the divergence + root cause as deferred.

## SCENARIOS / MEASURE_ONLY after this change

```bash
SCENARIOS="…existing 39… t2_eclk2 t2_clkcor t2_clkcor2 t2_waverot t2_wavez"   # 44
MEASURE_ONLY="t2_eclk t2_nsfrom t2_nsto t2_toinv t2_noclk"                     # 5
PROBE_ONLY="t2_grpn t2_wave3 t2_wave4 t2_waverev"                              # unchanged
```

- `two_clock.reference.json` grows 39 → 44 blocks; pre-existing blocks
  byte-identical (the hardened collector emits identical rows when both
  corners exist).
- `evaluated == 45` in `test_sta_clocks_match_opensta`.
- Golden `null` fields appear only in `t2_clkcor*` blocks (if the probe
  confirms per-corner anchoring) — the compare test handles them per
  `api.md` R1.

## Recording

- Commit all new/changed `t2_*.sdc` + `probe_*.txt` (and the folded
  golden) with the implementation — `< 1 MB` plain text/JSON.
- `docs/results.md` continuation of the 2026-11-20 entry: every probe
  line verbatim (STA codes included), the discriminator read-off, the
  rule implemented or "no code change".
- `docs/multi-clock.md` §1.7: extend the empty-collections bullet with
  the newly-evidenced forms; add the per-corner `-clock` anchor rule;
  cite `t2_eclk2` for the no-arg rule (replacing the ad-hoc mention).

## Files

- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_eclk.sdc`
- `tests/designs/sta_clocks/t2_eclk2.sdc` (new)
- `tests/designs/sta_clocks/t2_clkcor.sdc` (new)
- `tests/designs/sta_clocks/t2_clkcor2.sdc` (new)
- `tests/designs/sta_clocks/t2_waverot.sdc`
- `tests/designs/sta_clocks/t2_wavez.sdc`
- `tests/designs/sta_clocks/two_clock.reference.json`
- `tests/designs/sta_clocks/probe_t2_*.txt`
- `docs/results.md`
- `docs/multi-clock.md`
