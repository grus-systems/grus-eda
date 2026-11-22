# Implementation & verification steps

## Step 1 — Scenario files and `generate_ref.sh`

Create the 23 SDC files from `probes.md` (copy an existing `md_*` file and
replace the tail command lines; `pmo_*` copy a `pmd_*` file). `mdo_mindl`
and `mdo_minhu` change one preamble value each — leave a comment on the
edited line, same style as the `d_in` `-min` comment. Append the names to
`SCENARIOS` per `probes.md` section 6.

## Step 2 — Regenerate the golden

From the repo root, run the committed script as written:

```
bash tests/designs/sta_exceptions/generate_ref.sh
```

It re-invokes itself inside `ghcr.io/librelane/librelane:3.0.16` (docker via
Colima; start with `mise exec docker-cli@latest colima@latest lima@latest --
colima start --memory 12 --cpu 8` if needed). Every value in
`exceptions.reference.json` must come from that run — never edit by hand.
`git diff` must show the existing 72 entries unchanged and only the 23 new
keys added; if an existing entry moved, something else changed — stop and
investigate before committing.

## Step 3 — Read the formulas off the golden

Work through `probes.md` section 7 and write down, before touching code:

- max-side req (ideal and propagated anchor);
- min-side req constant and its decomposition into SDC terms;
- precedence outcomes (`mdo_fp` removal, `mdo_mc*` req, `mdo_both`
  per-side values).

## Step 4 — Implement the port branch in `analysis.rs`

Inside `run_sta_partitioned`'s composition loop:

1. Extend `delay_supported` (~line 3182) so `ExceptionEndpointKind::OutputPort`
   is supported; `Check { latch: false }` keeps its arc check. Update the
   comment above it — output ports no longer "keep their clock-check
   values"; clock gates, latch-D and arc-less checks still do.
2. Restructure the `delay_max_n` block (~line 3194): for `Check`, keep the
   existing `check_by_pin`-guarded capture formula unchanged; for
   `OutputPort`, replace each `pols.max[pol]` candidate's `req`/`slack` with
   the Step-3 formula (e.g. `req = n - odelay_max - setup_uncertainty`,
   adding whatever anchor `pmo_*` measured), keeping `arr`/`slew`.
   `sdc.output_delay_ns` / `sdc.output_clock_latency_ns` take the port name
   via `cx.port_name(PinId(ep))`.
3. Same for `delay_min_n` (~line 3239): `Check` keeps `w.req += n` (pre-MCP
   base + N); `OutputPort` sets `w.req` to the Step-3 composition and
   recomputes `w.slack = w.arr - w.req`. The MCP block is already gated by
   `delay_*_n.is_none()`, so delay-over-MCP needs no new code.
4. `unsupported_path_delay_endpoints` then counts only clock gates,
   latch-D and arc-less check endpoints — no counter change needed, the
   `delay_supported` gate does it.

Keep changes inside the loop; no signature, selector or `collect_endpoints`
edits.

## Step 5 — Test strict, then prove it can fail

1. `cargo test -p grus-core test_sta_exceptions_match_opensta` — all 91
   non-error scenarios at the existing tolerances (rel 1 %, abs 5 ps). No
   tolerance edits; a real divergence is fixed in composition or recorded
   as a measured fixture/model limit in `docs/results.md`.
2. Fail-proof (AGENTS.md lesson 7): mutate the new arm — e.g. keep the
   `latency + period` terms in the max req, or add `n` to the ordinary min
   req instead of replacing — and rerun: `mdo_*`/`pmo_*` must go red.
   `mdo_from0` goes red if the delay check fires on an uncovered `-from`;
   `mdo_fp` goes red if removal breaks. Restore the code.
3. Also run `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`.

## Step 6 — Docs, version, commits

- `docs/path-delays.md`: move the section-1 output-port paragraph into the
  measured semantics with the final formulas (including the propagated
  anchor and the min-side constant decomposition, naming the scenarios that
  proved each); update section 3 — remove `OutputPort` from the diagnosed
  endpoint list and the output-port bullet from "explicitly out of scope";
  update section 4's scenario count and coverage note (state plainly that
  the port-side multi-launch winner is exercised only per-transition — one
  launch reaches `y`); mark section 5 item 2 resolved.
- `docs/results.md`: dated entry — scenarios added, the regen command run,
  measured formulas and constants, the `mdo_from0` negative result,
  precedence outcomes, pass/divergence per probe, the fail-proof result.
- `docs/roadmap.md` (~line 270): drop "output ports" from the
  out-of-model endpoint list and from the "Remaining" bullet's section-5
  pointer (keep two-driver/latch fixtures and the other open items).
- `docs/timing-exceptions.md`: nothing currently claims path delays skip
  output ports — check the `set_min_delay`/`set_max_delay` bullet (~line
  251) still reads true after the change and adjust only if stale.
- `Cargo.toml`: `[workspace.package] version` 0.1.88 -> 0.1.89 in the same
  commit as the `analysis.rs` change (Cargo.lock follows via build).
- Suggested commit split, each committed when green: (1) `test:` the 23
  scenario files + `SCENARIOS` + regenerated `exceptions.reference.json`
  (the golden lands before the implementation, per the "commit the probe
  before implementing" pattern from the exceptions work); (2) `feat:` the
  port composition + version bump + doc comment updates; (3) `docs:`
  path-delays/results/roadmap updates — or fold into (2) if small.

## Verify commands (workflow.toml)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Risk checklist

- **Min-side constant ambiguous** — `mdo_min0` alone cannot decompose it
  (delay_min = hold_unc = 0.1 cancel); `mdo_mindl`/`mdo_minhu` are in the
  set for exactly this. If the value lands outside the decision table,
  record it and re-probe — do not fit by guessing.
- **Propagated anchor unresolvable on this fixture** — `-source` latency and
  the propagated `clk` port-pin arrival coincide within tolerance; record
  the measured anchor honestly rather than naming a mechanism.
- **`mdo_from0` shows a delay check** — would falsify the launch-source
  coverage model at ports; stop and re-derive before implementing.
- **`mdo_max3` shows the ordinary req** — OpenSTA keeps the tighter check;
  composition must then only replace when the delay check wins, contradicting
  the FF-side replace semantics — flag, don't implement around it silently.
- **Multi-launch winner unexercised** — `y` has exactly one launch; the
  winner rule is exercised per-transition only. Record in the docs; do not
  claim multi-launch verification.
- **Container/PDK** — docker via Colima; PDK from `~/.ciel` (`PDK_ROOT`
  override supported by the script).
- **Regen shifts an existing entry** — a defect (nothing input-side
  changed); investigate before committing.
