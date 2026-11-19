# API design — readers/saif.rs, power.rs, main.rs

## `crates/grus-core/src/readers/saif.rs` (new)

Mirror `vcd.rs`: parse function + `load_*` path wrapper, `anyhow` errors at
the boundary.

```rust
/// Per-signal SAIF activity fields (durations in TIMESCALE units).
pub struct SaifFields {
    pub t0: f64, pub t1: f64,
    pub tx: f64, pub tz: f64, pub tb: f64,
    pub tc: u64, pub ig: u64,
}

/// One scope in the SAIF hierarchy (`INSTANCE` node).
pub struct SaifScope {
    pub name: String,
    /// Signal groups flattened (NET and PORT lists merge here); names raw,
    /// escapes stripped.
    pub signals: Vec<(String, SaifFields)>,
    pub instances: Vec<SaifScope>,
}

pub struct SaifActivity {
    pub roots: Vec<SaifScope>,
    /// DURATION * TIMESCALE, seconds.
    pub duration_s: f64,
    pub timescale_s: f64,
}

/// What scope resolution could not map; reported, never silently dropped.
pub struct SaifDiagnostics {
    pub unmatched_scopes: usize,
    pub unmatched_signals: usize,
}

pub fn parse_saif(text: &str) -> Result<SaifActivity>;
pub fn load_saif(path: &Path) -> Result<SaifActivity>;
```

Tokenizer: `(`, `)`, atoms, `"quoted"`, `\`-escaped names. One pass; the
grammar in `saif-format.md` is small enough for a hand recursive-descent
parser like the VCD reader's.

### Scope selection + resolution

```rust
impl SaifActivity {
    /// Resolves the hierarchy to per-net edge counts for `design`.
    /// `scope`: `/`-joined instance path from a SAIF root (e.g. "tb/dut");
    /// `None` selects the outermost scope containing signal groups
    /// (auto-descend through single-child chains).
    pub fn resolve(
        &self,
        design: &DesignDb,
        scope: Option<&str>,
    ) -> Result<(VcdActivity, SaifDiagnostics)>;
}
```

Rules (measured semantics are `saif-format.md`; these are *our* rules):

- At the selected scope: signal name → `design.net_names` lookup; else
  top-level port name → its net. Each hit yields `(rise, fall)` from `tc`:
  even → `tc/2` each; odd → `rise` gets the extra edge iff `t0 >= t1`,
  else `fall` (odd TC ⇒ start ≠ end; the majority-time state is taken as
  the start state, and a low start's unpaired toggle is a rise). A duty
  **tie** (`t0 == t1`) resolves to `rise` — the start-low assumption —
  and is golden-covered: `clk` on the truncated-window `mb.saif` carries
  `TC=199, T0=T1=990` → `(100,99)`, the in-window truth. A heuristic
  bounded at ±1 edge per net — see `overview.md` §Rise/fall split.
- Nested `INSTANCE name`: `design` cell by name; each signal →
  `<inst>/<pin>` pin lookup → `design.pin_nets[pin]` → net. Unknown instance
  → `unmatched_scopes += 1`; unknown signal/pin → `unmatched_signals += 1`.
- Signals already on a resolved net: last wins (mb.saif's pin and net views
  of `clk`/`q` carry identical fields).
- `--saif-scope` path components that match no `INSTANCE` → hard error
  listing available paths (loud, not "annotated nothing").
- Missing `DURATION`/`TIMESCALE` → error. `duration_s <= 0` → error.

## `crates/grus-core/src/readers/mod.rs`

`pub mod saif;` + `pub use saif::*;` (alphabetical, matches existing style).

## `crates/grus-core/src/power.rs`

- `estimate_power` unchanged — still takes `&VcdActivity`; the SAIF path feeds
  it the resolver's output. Power math untouched.
- `PowerDiagnostics`:
  - rename `driven_nets_without_vcd` → `driven_nets_without_activity`
    (source-neutral honesty; regenerates `mb.power.json` in the same commit);
  - add `saif_unmatched_scopes: usize`, `saif_unmatched_signals: usize`
    (0 on the VCD path; serialized like the other fields).
- No new fields needed on `VcdActivity` — duty is carried in `SaifFields`
  only, used for the odd-TC split, documented as not consumed by the power
  model (flat leakage).

## `crates/grus-eda/src/main.rs`

```rust
#[arg(long, help = "Path to VCD (.vcd) switching-activity dump",
      conflicts_with = "saif", required_unless_present = "saif")]
vcd: Option<PathBuf>,

#[arg(long, help = "Path to SAIF (.saif) switching-activity file")]
saif: Option<PathBuf>,

#[arg(long, requires = "saif",
      help = "SAIF instance scope path (e.g. tb/dut); default: \
      outermost scope containing signal groups")]
saif_scope: Option<String>,
```

Power command body: replace `load_vcd(&args.vcd)` with a match — VCD path
unchanged; SAIF path does `load_saif` → `resolve(&design, args.saif_scope)`
→ merge `SaifDiagnostics` into `report.diagnostics` before the summary
print/JSON write. Print line becomes source-neutral
(`"activity window {:.3} s"`), and the diagnostics `eprintln!` gains the two
SAIF counts plus a source-neutral rename of `driven nets without VCD
activity` → `driven nets without recorded activity`. `Power` command doc
comment → "from VCD or SAIF switching activity".

## Tests

- `saif.rs` unit tests (no PDK): header/timescale/duration, nested scopes,
  NET vs PORT groups both collected, escaped names (`a\[0\]` → `a[0]`),
  fractional-TC error, missing-DURATION error, odd-TC duty split
  **including the `t0 == t1` tie → `(100,99)` case**, auto-scope
  single-chain descent + ambiguous → error.
- `power.rs` integration test `test_saif_power_mb_matches_opensta`
  (`test_support` helpers; fails loudly on missing PDK/files): load
  mb.def/mb.sdc/lib/lef → `sta_pin_slews` → `load_saif(mb.saif)` → resolve
  → `estimate_power` → deserialize `mb.saif.reference.json` `power_saif`
  block and assert every group field at the named tolerance; assert
  `saif_unmatched_*` counts; **per-net** compare the resolved SAIF activity
  against `load_vcd(mb.vcd)` — every net's `(rise, fall)` identical except
  `clk` = `(vcd.rise, vcd.fall - 1)` (the `#2000` fall is outside the
  `[10,1990]` SAIF window; the report-level delta is recorded in
  `results.md`, see `fixture.md` §Test).
- Tolerance: tightest honest per-field bounds measured during
  implementation (current: internal 2%, switching 5%, total 1%, leakage
  25% with a 1.5e-12 W abs floor — see `overview.md` and `probe.md`).
