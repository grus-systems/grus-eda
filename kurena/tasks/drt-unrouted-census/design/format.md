# drt-unrouted-census — Output format

## Line grammar

Inside the `== drt emit:` block, after `unrouted_kinds` /
`unrouted_blockers` (unchanged), the per-net section becomes:

```
  unrouted <net-name>
    fail <pin-label> <FailKind>[ region_escape]
    fail <pin-label> <FailKind>[ region_escape]
  unrouted <net-name2>
    fail <pin-label> <FailKind>
```

- `<net-name>` — `guide_net_name(&design_db, u.net)`, the same
  spelling emit writes into `EmitReport.unrouted` and step-E joins
  against the DEF.
- `<pin-label>` — `<inst>/<pin>` for instance pins
  (`pin_cells[pin] = Some(cell)` → `cell_names[cell]` +
  `pin_names[pin]`), the bare port name for io pins. Identical to
  the `stub_outside_cover` label convention and `pin_label` in
  `tests/drt.rs`; factor it into one `fn pin_label(db, pin)` in
  `main.rs` used by both this block and the existing
  `stub_outside_cover` print (which inlines the same logic today).
- `<FailKind>` — `NoPath` | `Unattachable` (`{:?}` — same spellings
  as the `unrouted_kinds` keys).
- ` region_escape` — present iff `(u.net.0, p.0) ∈
  graphs.region_escape` (a `BTreeSet<(u32, u32)>` built once before
  the loop).

## Ordering

- Nets print in `route_db.unrouted` order — identical to
  `report.unrouted` order (emit pushes `guide_net_name(u.net)` per
  `u` in sequence), so the `unrouted` line sequence is unchanged
  vs today.
- `fail` lines print in `u.failed` order, which is `pin_nodes`
  (= `net_pins`) order by construction of `ordered_failed`.
  Deterministic — no sort needed, none added.

## Constraints (why nested lines, not inline)

Two committed consumers anchor the `unrouted` line's exact shape:

- `tests/designs/check_drt_ours.sh` step-E extracts the golden's
  `ours_router.unrouted` via `re.findall(r"^\s+unrouted (\S+)$",
  elog, re.M)` — the `$` requires the name to be the last token.
- `tests/designs/recount_unrouted.py` classifies cli lines with
  `parts[0] == "unrouted" and len(parts) == 2`.

Any suffix on the `  unrouted <net>` line breaks both (the step-E
`unrouted` list would silently empty, tripping the loud
census-vs-DEF join downstream). So the `unrouted` line stays
exactly two tokens and per-pin detail nests at 4-space indent —
the same convention `    \S+ \[…]` self-drc finding lines already
use.

## Keep-list interplay

`check_drt_ours.sh` step-E's `cli_lines` keep-list is prefix-based
(`s.lstrip().startswith(…)`). `    fail …` matches none of the
existing prefixes today, so it would be dropped; `consumers.md`
adds `s.lstrip().startswith("fail ")` so regenerated goldens carry
the evidence. Prefix check at design time: no existing emit line
starts with `fail ` (`println!` audit of `main.rs`).

## Sample (gcd, illustrative — real numbers recorded at run time)

```
== drt emit: gcd ==
guided=269 routed=188 unrouted=81 projected=9 …
unrouted_kinds NoPath=46 Unattachable=52
unrouted_blockers l0ObsPin=1 l1Obs=4 l1Net=39
  unrouted _029_
    fail _073_/A2 Unattachable
    fail _075_/B1 NoPath
  unrouted resp_rdy
    fail resp_rdy Unattachable region_escape
  stub_outside_cover _165_ _182_/A
  …
```

`unrouted_kinds` stays the aggregate; the sum of all `fail` lines
across nets equals `NoPath + Unattachable` — a self-checkable
invariant for the results entry (`steps.md` step 4).

## Files

- `crates/grus-eda/src/main.rs`
