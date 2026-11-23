# e7-drc-e2e — `grus-eda drc` CLI

New `Commands::Drc(DrcArgs)` arm in `crates/grus-eda/src/main.rs`,
mirroring `congestion`/`irdrop` conventions: `load_lef_tech_db` for
the tech (its skip summary still prints), `parse_def` with `None`
lib_db (DRC needs netlist geometry only), `timing::phase` spans, plain
`println!` reporting.

## Args

```rust
#[derive(Args, Debug)]
struct DrcArgs {
    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: PathBuf,
    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: PathBuf,
    #[arg(long, help = "Path to routed DEF (.def) file")]
    def: PathBuf,
    #[arg(
        long,
        help = "<design>.drc.reference.json; prints per-rule deltas vs magic_per_rule"
    )]
    reference: Option<PathBuf>,
    #[arg(long, help = "Write the full DRC report (per-rule findings + bboxes) as JSON to this path")]
    json: Option<PathBuf>,
}
```

No `--lib` (unneeded), no `--strict`, no `--out` alias. Invocation:

```
grus-eda drc --tlef .../sky130_fd_sc_hd__nom.tlef \
             --lef  .../sky130_fd_sc_hd.lef \
             --def  tests/designs/gcd/gcd.routed.def \
             --reference tests/designs/gcd/gcd.drc.reference.json \
             --json /tmp/gcd.drc.ours.json
```

## Arm flow

1. `phase("drc.load.lef")` → `load_lef_tech_db`; `phase("drc.load.def")`
   → `parse_def(&args.def, &tech_db, None)` — record `elapsed_load_ms`.
2. `phase("drc.assemble")` → `drc::geom::assemble` — `elapsed_assemble_ms`.
3. `phase("drc.check")` → `drc::report::run_all(&drc, &tech_db,
   &design.strings)` — `elapsed_check_ms`.
4. Print the report (below). `--reference`: parse the JSON (loud
   `anyhow` on read/parse failure, irdrop-style), `magic_deltas`,
   print the delta table + unclaimed list. `--json`: write the
   serialized CLI report pretty-printed.
5. `timing::print_report()` behind `timing::enabled()`, as everywhere.

## Stdout layout

```
Loading technology from <tlef> and <lef>
Loading DEF from <def>
DRC <design>: 50 rules, 0 findings (assemble=..ms, check=..ms)
  rule      family     layer bound candidates findings
  li.c1     width      li1      170        869        0
  met2.2    spacing    met2     140     123456        0
  ...
claim boundary: 55 of 278 Magic drc(full) statements implemented (50
unique rules); 223 magic_only statements (wells, diffusion, taps,
poly, FETs, caps, angle rules) are not checkable from LEF+DEF and are
not claimed — 0 findings is not a full-deck clean result
```

- The per-rule table prints all 50 outcomes (`rule_name`-order within
  each family as emitted — table order), same census shape as the
  integration tests print. `layer`/`bound` show `-` when `None`.
- Nonzero findings print under their rule line, one per line:
  `    met2.2 [10520,48450,10590,48950] x [10350,48450,10420,53210] gap=(170,0) bound=140 nets=_001_,_000_`
  (DBU, `[xl,yl,xh,yh]`). No cap — findings are rare by construction.
- The claim-boundary line always prints, from
  `report::claim_boundary()`; `--reference` present → the numbers are
  recomputed from the loaded `magic_rules_checked` (and the function
  takes the derived counts — see below), absent → pinned constants.

`claim_boundary()` gains a `FromGolden` variant so the CLI derives the
split from the reference when one is loaded:

```rust
pub fn claim_boundary_from(deck: usize, le_stmts: usize,
                           le_rules: usize, magic_only: usize) -> String;
pub fn claim_boundary() -> String; // pinned-constant call of the above
```

With `--reference`, also:

```
deltas vs <path>:
  rule      ours magic
  met2.2       0     0
  ...           (50 rows; today all 0=0)
unclaimed (magic_only / unmodeled, ours not consulted):
  N-well spacing < 1.27um (nwell.2a): 120
  N-well width < 0.84um (nwell.1): 6
  All nwells must contain metal-connected N+ taps (nwell.4): 300
magic_total=305 (includes unclaimed master-internal findings; not comparable)
Runtime: load=..ms assemble=..ms check=..ms
```

## JSON schema (`--json`)

Wrapper over `SubsetReport` (serde derives already on the report types):

```json
{
  "design": "gcd",
  "input_def": "tests/designs/gcd/gcd.routed.def",
  "claim_boundary": "55 of 278 ... not claimed",
  "claim": {
    "implemented_rules": 50,
    "implemented_statements": 55,
    "deck_statements": 278,
    "deck_unique_names": 239,
    "magic_only_statements": 223
  },
  "rules": [ /* SubsetReport.rules verbatim */ ],
  "reference": {
    "path": "...",
    "magic_total": 305,
    "deltas": [{"rule": "met2.2", "rule_name": "Metal2 spacing < %d (met2.2)", "ours": 0, "magic": 0}],
    "unclaimed": [{"key": "N-well spacing < 1.27um (nwell.2a)", "magic": 120}]
  },
  "runtime_ms": {"load": 0.0, "assemble": 0.0, "check": 0.0}
}
```

`reference` is `null` without the flag; `claim` carries the derived
split when a reference is loaded, the pinned constants otherwise.
The struct lives in `main.rs` (CLI artifact), serde-serialized
directly — `serde`/`serde_json` are already `grus-eda` deps.

## Exit status

`Ok(())` always on a completed run, findings or not; `anyhow` errors
propagate for missing/unparseable inputs — consistent with every
other arm.
