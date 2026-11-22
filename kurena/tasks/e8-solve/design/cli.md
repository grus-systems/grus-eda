# e8-solve — CLI surface

Add one Clap subcommand:

```rust
/// E8: Sparse DC IR-drop solve on extracted PDN meshes
#[command(name = "irdrop")]
Irdrop(IrdropArgs),
```

The argument shape mirrors `power` so the estimator receives the same
Liberty/SDC/activity/parasitics semantics:

```rust
struct IrdropArgs {
    #[arg(long)] lib: PathBuf,
    #[arg(long)] tlef: PathBuf,
    #[arg(long)] lef: PathBuf,
    #[arg(long)] def: PathBuf,
    #[arg(long)] sdc: PathBuf,

    #[arg(long, conflicts_with = "saif")]
    vcd: Option<PathBuf>,
    #[arg(long)] saif: Option<PathBuf>,
    #[arg(long, requires = "saif")]
    saif_scope: Option<String>,

    #[arg(long, value_enum)]
    parasitics: Option<ParasiticsMode>,
    #[arg(long, default_value = "met2")]
    wire_layer: String,
    #[arg(long)] spef: Option<PathBuf>,

    /// <design>.irdrop.reference.json; supplies source clamps and deltas.
    #[arg(long)] reference: PathBuf,
    /// Directory receiving voltage CSVs and the summary JSON.
    #[arg(long)] out_dir: PathBuf,
}
```

`--reference` is required in E8 because source clamp values must come
from golden data, not a typed `1.80` or an invented default. The solve
API remains reusable without the JSON.

## Load and solve flow

1. `load_liberty(--lib)`.
2. `load_lef_tech_db(--tlef, --lef, interner)`.
3. `parse_def(--def, tech_db, Some(lib_db))`.
4. `load_sdc(--sdc)`.
5. Build the selected parasitics exactly as `power` does
   (`zero-wire` default, `placement`, or `spef`).
6. `sta_pin_slews(design, lib, sdc, parasitics)`.
7. Load VCD or resolve SAIF exactly as `power`; default to
   `VcdActivity::default()`.
8. `estimate_power`, merge `saif` diagnostics.
9. Convert `report.instances` to `cell_power_w[CellId.0]`.
10. Read the reference JSON once. For each `PdnNet`:
    - find `nets.<name>.conventions.embedded`;
    - read `supply_voltage_v`, `total_power_w`, scalar voltages, and
      `csv_rows`;
    - missing net/convention/field is an error naming the JSON path.
11. Compute `current_scale_v`:
    - POWER+GROUND present: `abs(VPWR_supply - VGND_supply)`;
    - otherwise: `abs(report.voltage_v)`;
    - fail if non-finite or `<= 0`.
12. `build_mesh`, `solve_pdn_net`, `voltage_rows`,
    `write_voltage_csv`, collect `IrDropNetReport`.
13. Write `<design>.irdrop.summary.ours.json`, print the per-net
    summaries.

The power loading block can be factored into a helper shared by
`Commands::Power` and `Commands::Irdrop` only if it stays mechanical.
Duplicating the existing `power` load sequence is preferable to a large
CLI refactor in this ticket.

## Commands to record

Build once so `/usr/bin/time` measures the tool rather than cargo:

```bash
cargo build -p grus-eda --release
```

gcd (macOS `/usr/bin/time -l`; Linux equivalent `/usr/bin/time -v`):

```bash
/usr/bin/time -l target/release/grus-eda irdrop \
  --lib "$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib" \
  --tlef "$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef" \
  --lef "$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef" \
  --def tests/designs/gcd/gcd.placed.def \
  --sdc tests/designs/gcd/gcd.sdc \
  --reference tests/designs/gcd/gcd.irdrop.reference.json \
  --out-dir tests/designs/gcd
```

picorv32:

```bash
/usr/bin/time -l target/release/grus-eda irdrop \
  --lib "$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib" \
  --tlef "$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef" \
  --lef "$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef" \
  --def tests/designs/picorv32/picorv32.placed.def \
  --sdc tests/designs/picorv32/picorv32.sdc \
  --reference tests/designs/picorv32/picorv32.irdrop.reference.json \
  --out-dir tests/designs/picorv32
```

No `--vcd`/`--saif` is passed for the golden comparison: the reference
script also used no annotation dump, so both estimators see their default
activity model. Explicit activity remains supported for standalone use.

## Output naming

Inside `--out-dir`:

```text
<design.design_name>.irdrop.voltage.ours.<net lowercase>.csv
<design.design_name>.irdrop.summary.ours.json
```

`design.design_name` comes from DEF (`gcd`, `picorv32`). `out_dir` is
created if needed.

## Reporting requirements

Each `IrDropNetReport` contains enough for `docs/results.md` without
rerunning:

- mesh counts;
- injected current/power and uninjected power;
- embedded reference `total_power_w`, derived reference current
  (`total_power_w / current_scale_v`), and delta;
- average/worst voltage and drop/bounce;
- residual absolute/normalized;
- source clamp max error;
- solver method/iterations and elapsed solve time.

Process runtime and peak RSS are not reimplemented in Rust for this
ticket. `/usr/bin/time` supplies them, and `docs/results.md` records the
exact time command and `maximum resident set size`/`Maximum resident
set size` value.

## CLI failure cases

The command errors before writing artifacts on:

- reference JSON missing the current design's PDN net or embedded block;
- invalid/non-positive current scale;
- mesh validation violations;
- non-finite instance power (negative is legal — Liberty
  internal-power artifact, counted and reported on stderr);
- sink-pin/cell mapping mismatch;
- Cholesky failure;
- output path or JSON serialization failure.

Partial output is possible if a later net fails after an earlier CSV was
written. The error is left explicit; E8 does not add transactional
cleanup machinery.

## Files

- `crates/grus-eda/src/main.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/gcd/gcd.irdrop.summary.ours.json`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/picorv32/picorv32.irdrop.summary.ours.json`
