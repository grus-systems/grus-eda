# Reporting — the named escape attempt

## §1 `fail`-line format

`crates/grus-eda/src/main.rs` ~L2188–2205 currently joins
`graphs.region_escape` into a ` region_escape` suffix. Extend the
same join: build `BTreeMap<(u32, u32), &EscapeRecord>` from
`graphs.escapes`, and append one more suffix token.

```
    fail <label> <Kind>[ region_escape][ escape=<token>]
```

Token shapes (single whitespace-free token, `escape=` prefixed):

```
escape=halo                    — reached via halo (span == 0)
escape=halo(span=2)            — reached via halo + corridor
escape=corridor_bound(span=31) — refused: gap exceeds ESCAPE_CORRIDOR
escape=corridor_masked         — refused: every corridor died at the mask
escape=halo_empty              — no legal track in the halo on any layer
```

Every `EscapeKind` renders as one `escape=` token; the parenthesized
fields are deterministic integers. A failed pin with an `escapes`
record always shows it; a pin with `region_escape` but no record can
only happen when the mechanism was off — print no suffix (the flag
alone remains the honest report).

Example (post-mechanism gcd, if `_301_/B1` still fails):

```
    fail _301_/B1 Unattachable region_escape escape=corridor_masked
```

## §2 Consumer updates

`tests/designs/recount_unrouted.py` (~L185–200) parses `fail` lines
strictly — `len(parts) == 4` requires `parts[3] == "region_escape"`,
anything else is `malformed`. Update:

- accept optional `parts[4]` matching `^escape=\S+$` (no further
  semantic join — the token is evidence text);
- update the docstring line describing the format (currently
  "`fail <inst>/<pin> <Kind>[ region_escape]`").

`tests/designs/check_drt_ours.sh` keep-list (~L937) already keeps
`fail ` lines verbatim — the suffix lands in `cli_lines`
automatically; no script change needed. No new top-level census
line is added (deliberate: keeps `cli_lines` shape and the keep-list
byte-stable; escape outcomes for *routed* pins are graph-side data
asserted by tests, not emit text).

## §3 Golden/golden-adjacent surfaces

- `gcd.drt.ours.routed.def` — byte-compared by
  `test_e2e_ours_def_deterministic_gcd`. If any gcd route moves
  (expected: `_134_`/`_138_` recover or shift tie-breaks), this test
  goes red and the file is regenerated via
  `tests/designs/generate_drt_ours.sh gcd` (host leg, release
  binary — not the container). Record old/new sha in results.
- `gcd.drt.ours.reference.json` — `ours_router.cli_lines` embeds the
  committed `fail`/`unrouted` text. `test_e2e_ours_reference_gcd`
  joins the file *internally* (never re-routes), so it stays green
  even when the golden is stale vs. a fresh emit — name the
  staleness in `docs/results.md`; regeneration is the
  `check_drt_ours.sh` container leg (billable — ask; see steps.md
  §6).
- `route_digest` — schema untouched; value may drift with routes.
  `test_maze_determinism` asserts self-consistency only.

## §4 Test-surface updates (provenance joins)

- `test_pin_attachment`: today asserts escape pins have empty
  `pin_nodes`. Post-halo that arm changes — an escape pin may attach
  via halo nodes. New assertion: every pinned escape name still
  appears in `region_escape`; an escape pin with non-empty ids must
  have an `escapes` record with `kind == Halo`; an escape pin with
  empty ids must have a record with `kind != Halo`. Non-escape pins
  must have no `escapes` record.
- Graph census pins (`total_nodes`, `total_edges`,
  `via_edges_per_pair`, `total_via_edges`, possibly
  `transit_pairs`-adjacent counts): re-measure on the real runs and
  re-pin — halo pairs add nodes/edges for gcd's 17 escapes. Never
  hand-derive the new pins; print-then-pin (E0 §2).
- `test_graph_determinism`: include `escapes` in the equality
  (whole-struct derive already covers it if `RouteGraphs` derives —
  confirm at impl).
