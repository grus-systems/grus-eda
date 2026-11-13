# F7: make the CLI advertise only what works

## Goal
Remove or fix every CLI option that does not do what it says: delete the
four dead STA options, make the required STA inputs required in clap,
turn `--parasitics` into a typed enum, and fix the `--gcell-tracks`
sentinel and the hard-coded `--layer-adjust` vector. Design:
`docs/design/review-fixes.md` F7 and the Review resolution section;
review: the design review record (not included) Q2 and Q3.

## Files
- crates/grus-eda/src/main.rs
- docs/architecture.md
- Cargo.toml
- Cargo.lock

## Interface
- `StaArgs`: delete `json`, `verilog`, `clock_pin`, `period_ns`.
  `tlef`, `lef`, `def`, `sdc` become required `PathBuf` (not `Option`).
  The handler no longer bails with "sta requires ..."; clap reports
  missing arguments. `lib` stays required, `spef` and `wire_layer`
  stay as they are.
- `ParasiticsMode`: a new `#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]`
  enum with variants `None`, `Placement`, `Spef`, used as
  `parasitics: Option<ParasiticsMode>`. Mode resolution is a single
  match on `(args.parasitics, &args.spef)` preserving today's behavior
  exactly:
  - `(None | Some(None), None)` -> zero wire
  - `(Some(Placement), None)` -> placement RC
  - `(None | Some(Spef), Some(_))` -> SPEF
  - anything else -> the same `anyhow::bail!` style error as today,
    with the debug form of the enum in the message.
- `CongestionArgs::gcell_tracks`: `Option<u32>` with
  `value_parser` range `1..` and no default. Pass
  `args.gcell_tracks.map(|t| t as i32)` as the override.
- `CongestionArgs::layer_adjust`: keep `Option<f32>`; reject values
  outside `0.0..=1.0` with a clap value parser range. Build the
  adjustment vector in the congestion handler as length
  `tech_db.routing_layers.len()`: index 0 is `0.0`, all other indices
  are the given factor. (For sky130hd the length is 6, so behavior on
  the test designs is unchanged.)
- `docs/architecture.md` Section 5: rewrite the `grus-eda sta` synopsis
  to the real interface (required `--lib --tlef --lef --def --sdc`,
  optional `--parasitics`, `--spef`, and the previously omitted
  `--wire-layer`), and change the congestion synopsis
  `[--gcell-tracks 15]` to `[--gcell-tracks <n>]`.
- Bump `[workspace.package] version` patch in `Cargo.toml` (AGENTS.md
  Section 5) and refresh `Cargo.lock`.

## Acceptance
- `cargo build --release -p grus-eda`
- `cargo clippy --release -p grus-eda -- -D warnings`
- `target/release/grus-eda sta --help` shows no `--json`, `--verilog`,
  `--clock-pin`, or `--period-ns`, and `--parasitics` lists the enum values
- `target/release/grus-eda congestion --help` shows `--gcell-tracks` with
  no default of 15
- `target/release/grus-eda sta` (no arguments) fails in clap, not with
  the old "sta requires" message
- `target/release/grus-eda congestion --gcell-tracks 0 --tlef x --lef y --def z`
  is rejected by the parser (argument error, not a panic)

## Notes
- Read first: `AGENTS.md`, `docs/design/review-fixes.md` (F7 + Review
  resolution), the design review record (not included) Q2 and Q3.
- CLI-only ticket: do not change any algorithm in `grus-core`, any STA
  or congestion number, or any golden/reference file. `main.rs` is the
  only source file touched.
- The `place` doc comment and other subcommands are out of scope
  (Ticket 04 handles the "greedy" wording).
- Do not read anything outside this repository. Do not push.
