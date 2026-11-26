# Goal

E8 (IR drop / power-grid analysis) ticket 1: commit the
PDNSim reference artifacts and reproduction script on
gcd, per the scoping done-when
(`docs/roadmap.md` §E8, P-IRD measured on the pinned
image).

## Work

- New committed script
  `tests/designs/generate_irdrop_ref.sh` following the
  `generate_ref.sh`/`generate_cts_ref.sh` conventions:
  host wrapper that re-invokes itself inside
  `ghcr.io/librelane/librelane:3.0.16` with the repo at
  `/work` and PDK at `/pdk`, `-u $(id -u):$(id -g)`,
  Tcl safe for stdin (`== 0` negations, no `!`).
- Inside the container, on `gcd.placed.def` +
  `gcd.sdc` (+ liberty/tech-lef/cell-lef as the other
  flows read them):
  - `check_power_grid -net VPWR` and `-net VGND` —
    record the connectivity verdict verbatim.
  - `analyze_power_grid -net VPWR -voltage_file <csv>`
    and the same for `VGND` — the P-IRD-measured
    surface. Record whether `VGND` analysis reports a
    ground *bounce* (rise) or a drop and keep both
    semantics in the golden.
  - Pin the current-source convention, the E8
    done-when's open item. Generate **both**:
    (a) PDNSim's default embedded per-instance power
    estimate (the P-IRD 0.591 mV regime), and
    (b) explicit per-instance power via
    `set_pdnsim_inst_power`, fed from OpenSTA's own
    per-instance power report produced in the same
    container run (the reference-consistent source —
    NOT our N7 numbers). Probe the flag signature
    (`-help`/usage) in-script; abort loudly on an
    unmatched parser surface rather than defaulting.
  - Record which convention the E8 engine will mirror
    (expected: (b) explicit, since our N7 power emits
    per-instance power) — state the choice and its
    measured delta vs (a).
- Commit under `tests/designs/gcd/`:
  - `gcd.irdrop.reference.json` — per net and per
    convention: connectivity, total power, supply
    voltage, average voltage, average and worst IR
    drop/bounce, analyzed node/instance counts. All
    values emitted by the Tcl/script itself, never
    hand-written.
  - `gcd.irdrop.voltage.csv` (one file per convention
    if they differ, e.g. `gcd.irdrop.voltage.<conv>.csv`)
    — the per-instance voltage map, the criterion-2
    golden. Commit if < 1 MB; otherwise gitignore and
    record size + SHA-256 prefix.
- picorv32: parameterize the same script; include its
  artifacts (committed or gitignored with
  size+SHA record) if the run is clean, otherwise
  explicitly defer with reason.
- `docs/results.md` entry: script steps verbatim,
  measured values vs the P-IRD scoping numbers
  (total power 1.04 mW, supply 1.80 V, avg drop
  0.223 mV, worst 0.591 mV, 384+ CSV rows on gcd
  VPWR), artifact sizes, SHA-256 prefixes, rerun
  byte-reproducibility count.
- `tests/designs/README.md` artifact row;
  `docs/roadmap.md` §E8 status → in progress /
  reference committed.

## Done when

The reference set exists, is reproducible from the
committed script in the pinned image, the P-IRD
scoping numbers are confirmed or corrected in
`docs/results.md`, and the current-source convention
the engine will mirror is pinned with measured
evidence. No solver implementation in this ticket —
reference only.

## Files

- `tests/designs/generate_irdrop_ref.sh`
- `tests/designs/gcd/gcd.irdrop.reference.json`
- `tests/designs/gcd/gcd.irdrop.voltage*.csv`
- `tests/designs/picorv32/picorv32.irdrop.*` (if included)
- `docs/results.md`
- `docs/roadmap.md`
- `tests/designs/README.md`
