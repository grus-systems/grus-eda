# e8-pdn-parse — Golden cross-checks

Reference artifacts were committed by `e8-irdrop-ref` (`ff24949`) and
are regenerated only via `tests/designs/generate_irdrop_ref.sh` inside
`ghcr.io/librelane/librelane:3.0.16` (`sta`/`openroad` binaries, OpenDB
`check_power_grid` + PDNSim `analyze_power_grid`, `embedded`
convention). This ticket consumes them read-only — the CSVs and JSON
are never regenerated or reformatted here.

## Spec correction — assert this, not the literal wording

`acceptance.md` says the two CSVs are "= 384 rows total over both".
The committed files carry **384 data rows each** (header excluded;
`wc -l` reports 385 lines per file). The checkable rule is the
stronger per-net one:

```text
census(VPWR).attached_terminals == data_rows(gcd.irdrop.voltage.vpwr.embedded.csv) == 384
census(VGND).attached_terminals == data_rows(gcd.irdrop.voltage.vgnd.embedded.csv) == 384
```

Same for picorv32: 10,205 per net. The test deserializes the CSVs and
compares counts per net — never the aggregate, never a typed constant
alone (the file is the source of truth; the constant appears only in
`docs/results.md` as the recorded result).

## Per-assertion mapping

| acceptance item | assertion | golden source |
|---|---|---|
| `pdn_nets` carries VPWR+VGND rects/via stacks/attach table | exact `wire_rects`/`vias` counts per net; every `PdnVia` has resolved `lower`/`upper`/`cut`/`cut_count`; `attachments` non-empty | struct invariants |
| renamed via resolves through defs, not name parse | synthetic DEF with `+ LAYERS`-backed renamed via resolves to the right triple; LEF `+ VIA`-style fixture resolves via `via_defs` | `tests.md` |
| connectivity mirrors `check_power_grid` | `all_shapes_connected == true` **and** `unreachable_pins.is_empty()` per net, iff `reference.json` `nets.<N>.connectivity_result == "1"` | `gcd.irdrop.reference.json` |
| attached-terminal count per net | `attached_terminals == csv_data_rows` per net; **plus** `(instance, terminal)` name-set equality between `conn_pins`-expanded attach table and CSV rows | `*.embedded.csv` |
| picorv32 census | same connectivity/count asserts against its JSON + CSVs; census numbers recorded in `docs/results.md` | picorv32 goldens |

### `(instance, terminal)` set equality

CSV `Instance`/`Terminal` columns are odb iterm names — expect
unescaped DEF spellings matching `DesignDb` names. If picorv32 names
with escapes diverge, normalize at the comparison boundary only and
record the rule in `docs/results.md` — never in the model.

### CSV `Layer` column — observed anomaly, not asserted

`gcd.irdrop.voltage.vpwr.embedded.csv` prints `li1` for every
`dfxtp_2` `VPWR` row while the pin's PORT rect is on `met1`. The
column reflects PDNSim's chosen contact layer, not our `MacroPin`
layer — excluded from per-element assertions and noted in
`docs/results.md`.

### X/Y location check

CSV X/Y (µm) equals `pin_pos_dbu` (the `getAvgXY`-style truncated
center) — verified by spot check: `_411_` `VPWR` at `(12.65, 51.68)`
µm matches the stored center. The golden test asserts
`|ours_dbu − csv_um * dbu_per_micron| ≤ 1` DBU per attached pin. If
the tolerance trips on picorv32, the root cause is named in
`docs/results.md` before the check is loosened — an integer-DBU gap
is a bug per project lesson 5.

## What is *not* compared here

- Voltage/drop numbers (`worst`/`average` in the JSON, `Voltage` CSV
  column): outputs of the e8-mesh/e8-solve pipeline, not the parse
  stage. `e8-pdn-parse` stops at "all attached, all connected".
- JSON fields beyond `nets.<N>.connectivity_result` and
  `nets.<N>.zeroed_probe.csv_rows`: the census records ours side by
  side, but only the two named keys gate the test.

## Files

- `tests/designs/gcd/gcd.irdrop.reference.json` (read-only)
- `tests/designs/gcd/gcd.irdrop.voltage.vpwr.embedded.csv` (read-only)
- `tests/designs/gcd/gcd.irdrop.voltage.vgnd.embedded.csv` (read-only)
- `tests/designs/picorv32/` (same artifacts, read-only)
- `crates/grus-core/src/pdn.rs` (test module asserting the above)
- `docs/results.md`
