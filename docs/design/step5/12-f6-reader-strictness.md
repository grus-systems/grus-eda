# Step 5 ticket 12 — F6: readers reject malformed input

## Background

Review `docs/review-2026-11-11.md` §F6: "The repository rule is 'handle
errors at system boundaries'. The DEF reader does the opposite: numbers
are read with `.parse().unwrap_or(0)`, unknown keywords fall into
`_ => {}`, a `ROW` with too few words becomes a row at (0, 0) with zero
sites. A truncated or unusual DEF yields a plausible, wrong database."

Verified against the current code:

- `readers/def.rs`: ~20 `parse().unwrap_or(0)` sites on grammar-required
  numbers (DIEAREA / ROW origin and step counts, TRACKS, GCELLGRID,
  component and pin coordinates, VIA/geometry numbers), several `_ => {}`
  arms that swallow unknown keywords inside sections, and section-level
  dispatch that ignores unknown sections. Across the five designs'
  placed DEFs the section headers that actually occur are: VERSION,
  DIVIDERCHAR, BUSBITCHARS, DESIGN, UNITS, DIEAREA, ROW, TRACKS, VIAS,
  COMPONENTS, PINS, SPECIALNETS, NETS, END.
- `readers/lef.rs`: the same shape — `next_f32().unwrap_or(0.0)` on SITE
  size, layer WIDTH / RESISTANCE / CAPACITANCE / PITCH and similar
  grammar-required values (about a dozen sites), plus `_ => {}` arms.
- `readers/spef.rs`: number parsing already propagates errors with
  context (`with_context(at)?`) — check it for the remaining patterns
  (token-count assumptions that panic on short lines, silent skips) and
  fix only what is actually there.
- `readers/liberty.rs`, `readers/verilog.rs`, `readers/yosys_json.rs`:
  check for the same patterns; liberty in particular has
  `unwrap_or` sites — distinguish "value has a real default in the
  format" from "required number silently zeroed" and fix the latter.

The review also asks for a fuzz target per reader (`cargo fuzz`,
property: returns `Ok` or `Err`, never panics, never loops). This
environment has only the stable toolchain and no `cargo-fuzz`; do not
install toolchains. Instead, the no-panic property is proven by the
robustness tests under Acceptance, which run under plain `cargo test`.
Fuzz targets proper are out of scope for this ticket.

## Files

- Modify: `crates/grus-core/src/readers/def.rs`
- Modify: `crates/grus-core/src/readers/lef.rs`
- Modify if the check above finds real sites: `crates/grus-core/src/readers/spef.rs`,
  `crates/grus-core/src/readers/liberty.rs`,
  `crates/grus-core/src/readers/verilog.rs`,
  `crates/grus-core/src/readers/yosys_json.rs`
- Do not touch: golden files, `tests/designs/**` inputs, the database
  structures the readers produce (`design_db`, `lib_db`, tech DB types) —
  parsed results on valid input must be identical.

## Work

### DEF reader

- Every number the DEF grammar requires parses or the read fails with an
  error that names the line number and the field (existing error/context
  helpers in the file are the precedent). This replaces the
  `parse().unwrap_or(0)` sites in DIEAREA, ROW, TRACKS, GCELLGRID,
  COMPONENTS, PINS, NETS geometry and VIAS.
- A `ROW` (and each other fixed-shape record) with too few words is an
  error, not a zero-filled record.
- Unknown section headers are an error unless the section is on an
  explicit skip list in the code, with a comment saying why each skipped
  section is safe to ignore (content our database does not model). The
  list must cover what OpenROAD emits in our flows; sections we parse
  (list in Background) are not skips. Unknown keywords inside a parsed
  section: error for keywords in fixed-shape records we model; where a
  section legitimately carries properties/attributes we do not model,
  name them in the same explicit way rather than a bare `_ => {}`.

### LEF reader

- Same rule for grammar-required numbers: SITE width/height, layer TYPE
  routing values used by the tech DB (WIDTH, PITCH, RESISTANCE,
  CAPACITANCE, THICKNESS where read), macro/pin geometry we store. A
  missing or unparseable value is an error naming the construct.
- Benign defaults may stay only where the LEF spec itself defines the
  default; name each kept default in your report.

### SPEF and the rest

- Report what you found per file, including "already strict" where true.
  Fix token-indexing that can panic on a truncated line (bounds-checked
  access with a line-numbered error instead).

### Tests

- Per changed reader, unit tests that feed malformed input and assert an
  `Err` (not a zero-filled `Ok`): at minimum a bad number in a required
  field, a truncated fixed-shape record, and (DEF) an unknown section
  header.
- Robustness test: for at least the DEF and LEF readers, take a small
  real input available in the repo (a fixture under `tests/` or the
  sky130 tech LEF via `test_support` if present) and assert that parsing
  every prefix truncation either succeeds or errors — it must never
  panic. Keep the input small enough that the sweep runs in seconds; if
  no small real fixture exists for a reader, build one from a minimal
  valid document in the test.

## Acceptance

- All five designs still parse and the full test suite passes exactly as
  before (counts and golden comparisons unchanged): strictness must not
  reject any input OpenROAD/LibreLane actually emits.
- New malformed-input unit tests pass; the truncation sweep never panics.
- `cargo clippy --all-targets -- -D warnings` exits 0.
- Your report lists, per reader: every default you kept and why, every
  section on the DEF skip list and why, and anything you found that this
  ticket's scope did not let you fix.
