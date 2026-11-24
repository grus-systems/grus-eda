# e6-drt-e2e — `<d>.drt.ours.reference.json` schema (r3)

Emitted by `check_drt_ours.sh` step E inside the pinned container;
committed for both designs. `json.dump(indent=2) + "\n"` — the file
must byte-reproduce across checkouts, so nothing in it may carry an
absolute path or wall-clock field. Fields exempt from
byte-reproduction by construction (recorded, not asserted):
`image_id` (image digest of the run — gate asserts it is
non-null/non-empty, not its value), `input_def_sha_prefix`,
`*_sha256_16` stamps (they record, don't gate).

## Top level

```json
{
  "design": "gcd",
  "image": "ghcr.io/librelane/librelane:3.0.16",
  "image_id": "sha256:…",
  "generated_by": "tests/designs/check_drt_ours.sh",
  "input_def": "tests/designs/gcd/gcd.drt.ours.routed.def",
  "input_def_sha_prefix": "…16…",
  "input_def_size_bytes": n,
  "input_def_provenance": "grus-eda drt via tests/designs/generate_drt_ours.sh",
  "guide": {"file": "tests/designs/gcd/gcd.guide", "size_bytes": n, "sha256_16": "…"},
  "ours_router": { … }, "openroad": { … }, "magic": { … },
  "placed_baseline": { … }, "def_metrics": { … }, "delta": { … },
  "drc_reference": {"file": "…", "sha256_16": "…"},
  "drt_reference": {"file": "…", "sha256_16": "…"},
  "sanity_control": { … }
}
```

## `ours_router` — emit provenance

```json
{
  "grus_eda_version": "0.x.y",
  "guide": {"file": "…", "size_bytes": n, "sha256_16": "…"},
  "emit_log": "tests/designs/<d>/runs/drt-ours/<d>.drt.ours.log",
  "emit_log_present": true,
  "cli_lines": [ "== drt emit: gcd ==", "guided=269 routed=…",
                 "unrouted_kinds NoPath=… Unattachable=…",
                 "unrouted_blockers l1Obs=… l1Net=…",
                 "  unrouted <name>", "  area_residual …",
                 "  stub_outside_cover <net> <pin>",
                 "self_drc findings_total=0", "self_drc scope: …" ],
  "routed": n, "unrouted_count": n, "unrouted": ["…"],
  "unrouted_kinds": {"NoPath": n, "Unattachable": n},
  "unrouted_blockers": {"l1Obs": n, "l1Net": n, "l2Net": n} | null,
  "collisions": n, "area_patches": n, "area_residuals": n,
  "hull_masked": n, "notch_fills": n
}
```

`cli_lines` is the allowlisted prefix set — **`wrote routed DEF to
…` stays removed** (absolute checkout path) and a guard fails
assembly if any retained line carries `/`-absolute paths.

Field semantics:

- `unrouted`/`unrouted_count` — the connectivity cost, named per net.
- `unrouted_kinds` — decomposition by `FailKind`; `null` only if the
  emit binary predates the line (the emit and script are
  version-locked; a present census line with a missing kinds line is
  a `fail`, not a guess).
- `unrouted_blockers` — wave-death postmortem census `{l<L><Kind>: n}`
  (kind: Obs/ObsPin/Net/Pin); `null` when no blockers were
  attributable. Recorded, not gated — it is the access work's input
  data (`pin-access.md`).
- `stub_outside_cover` lines (in `cli_lines`) — the unattachable-pin
  census: pins that got no legal paint. **r3 redefines it** — it is
  the `Unattachable` diagnostic (r2's "nonzero is a bug" predates the
  access model). Target empty post-recovery; the gate asserts every
  entry's net appears in `unrouted` (`gate.md`).
- `hull_masked` — nodes removed per mask; `> 0` is a gate assertion.
- `notch_fills` — self-spacing fill rects emitted (new field,
  appended to the `guided=` census line — `self-spacing.md`;
  recorded `>= 0`, never a target to optimize toward).

## `openroad`

`read_def`/`surfaces` prove the DEF loads; `drt_report` is `null`
with `drt_report_source: "not_applicable_native_router"` — the
native router has no OpenROAD counterpart report (do not read this
as a missing check). Antenna fields mirror the baseline variant's.

## `magic`

- `version`, `style: "drc(full)"`, `rcfile` — deck identity.
- `count_flat`, `count_total` — Magic's `drc count` prints.
  **`count_flat` is recorded but is NOT a routing-level signal**:
  the mutation control leaves it at 0 because the flat counter
  misses hierarchy-boundary errors entirely. Never assert or claim
  `count_flat == 0` in docs — the gate reads `lef_expressible` /
  `per_rule` only (E7 counting-surfaces lesson).
- `per_rule` — raw rendered `drc listall why` name → count on ours.
- `lef_expressible` — the full 50-rule-id map, explicit zeros:

  ```json
  "li.3": {"ours": 0, "placed": 0, "routed_ref": 0, "inherited": true}
  ```

- `magic_only` — rows only for rule ids nonzero in ≥1 input:
  `{rule, ours, placed, routed_ref|null, inherited}` where
  `inherited = (ours == placed)` — the decomposition the gate
  asserts (ours findings must match the placed baseline class).
- `unclassified_fired` — `[{name, ours, placed, routed_ref}]`,
  `name` restricted to the two pinned allowlist strings (assembly
  aborts on any other).

## `placed_baseline` / `def_metrics` / `delta`

`placed_baseline` holds `{count_flat, count_total, per_rule}` for
the placed DEF — the inherited-class source. `def_metrics` parses
our emitted DEF (signal nets, wirelength µm, per-cut-layer via
census, `via_names_meaning`, `dbu_per_micron`) — same grammar as
`generate_drt_ref.sh`'s `routed_metrics`; `RECT` fill pieces count
as paint, not as wire segments or vias (note their count in the
metrics comment only if the parser reports them). `delta.vs_baseline`
/ `delta.vs_delegated` carry `{total_wirelength_um,
total_wirelength_pct, via_count_total, vias_per_cut_layer,
antenna_violations, antenna_pin_violations, signal_nets_routed}`
signed deltas — the `results.md` comparison table's source.

## `sanity_control`

`{mutation, count_flat, count_total, per_rule, new_rules_vs_clean,
in_deck, violated_rules_in_checked_deck}` — step D injects a probe
wire into a copy of our DEF; the deck must flag ≥1 new
`lef_expressible` rule vs clean. Once the golden reads clean,
`new_rules_vs_clean` is non-empty **by construction** (the WIP
golden had it empty because probe fragments joined the
already-firing `met2.2` — recorded, not a defect to chase).

## Determinism contract

Byte-reproduction is required across reruns in the same image and
across checkouts: all path fields are repo-relative, all counts
derive from Magic's stable `per_rule` output and the deterministic
emit, and `sort_keys` is not used — field order is fixed by
construction. If a rerun diffs, the diff is the bug — investigate,
don't re-commit (never rebaseline). `image_id` drift across pulls is
the one expected change; record the current digest, don't gate on it.

## Files

- `tests/designs/gcd/gcd.drt.ours.reference.json` (committed)
- `tests/designs/picorv32/picorv32.drt.ours.reference.json` (committed)
- `tests/designs/gcd/gcd.drt.ours.routed.def` (committed, <1 MB)
- `tests/designs/check_drt_ours.sh` (emitter)
