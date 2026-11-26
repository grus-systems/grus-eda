# drt-selfdrc-exit — census output design

## Verdict line

One line, printed immediately after the `guided=…` census line —
before `iter_progress=` and the unrouted listing (spec option A:
self-DRC visible before the unrouted list, adjacent to `routed=`):

```
self_drc_verdict <TOKEN>
```

Tokens (exactly one per `drt` run):

| Token | Printed when | Exit |
|-------|--------------|------|
| `CLEAN` | check ran, `findings_total = 0` | 0 |
| `VIOLATIONS findings=N tolerance=T` | check ran, `N > T` | 1 |
| `TOLERATED findings=N tolerance=T` | check ran, `0 < N <= T` | 0 |
| `SKIPPED (--no-self-drc; routed= counts connectivity, not DRC)` | check not run | 0 |

Rationale for the token set: `CLEAN`/`VIOLATIONS` are the fail-loud
pair the spec asks for; `TOLERATED` keeps a nonzero residual visible
rather than silently absorbed by the budget; `SKIPPED` exists so a
`--no-self-drc` census can never be misread as clean — without it a
skipped run prints exactly the clean-run `guided=` line.

## Block ordering (self-DRC enabled run)

```
== drt emit: gcd ==
guided=269 routed=189 unrouted=80 … hole_residuals=0
self_drc_verdict CLEAN
self_drc findings_total=0
self_drc scope: 50-rule lef_expressible subset on our emitted DEF — …
preemit_check findings_total=0            # only under --preemit-check
  rule=… (nonzero rules only)
preemit_check scope: …                    # rewritten text, below
iter_progress=…
iter_prov=…
pin_collision …                           # only non-empty
unrouted_kinds …
  unrouted <net> / fail <pin> …           # per-net listing
stub_outside_cover / *_residual lines
wirelength_um ours=… via.<cut> …          # --reference deltas
wrote routed DEF to …
```

The `guided=` line itself is **untouched** — no trailing field; the
`check_drt_ours.sh` step-E regex (`guided=(\d+) routed=…
hole_residuals=(\d+)`) and every consumer keep working.

## `preemit_check` scope line rewrite

Current text says findings "do not gate the write or the exit code
until drt-selfdrc-exit lands". After this ticket lands:

```
preemit_check scope: report-only — 50-rule lef_expressible subset on the
in-memory emit shape set (same input --self-drc re-parses); the exit
gate is self_drc on the emitted text
```

## Script interaction (`check_drt_ours.sh`, no edit)

Step E's keep-list matches `s.startswith("self_drc")`, so
`self_drc_verdict …` and the relocated `self_drc` block flow into
`ours_router.cli_lines` automatically — the verdict is pinned as
golden evidence. Committed goldens' `cli_lines` go stale in order and
content (new line + moved block) until the next container regeneration
run; the census *numbers* (`routed`, `unrouted_count`, …) are parsed
by anchored regexes that are unaffected. `recount_unrouted.py` and the
`fail`-line grammar are untouched.

## What stays exit-0

- `unrouted > 0` (gcd emits with `unrouted=80` today; connectivity
  gaps are named in the census, not gated by this ticket).
- `preemit_check` findings — report-only mirror of the same subset.
- `pin_collision`/`*_residual` survivors — already named census
  classes.
