# e6-drt-emit — Close-out steps

Implementation landed at WIP `dc8cdb4`; this is the run + docs +
verify sequence the rejection memo asked for. Order matters: the
detached picorv32 leg starts **first** (~135+ min release) so it
runs while everything else closes.

## 0. Small test additions (tests.md)

- Byte-stable check inside `emit_gate` (picorv32 gets the
  write → parse → write → identical proof too).
- `drt` CLI parse assert in `main.rs` `mod tests`.
- Re-confirm the `#[ignore]` boundary: `test_emit_selfdrc_picorv32`
  (and `test_maze_census_picorv32`, `test_maze_determinism_picorv32`)
  stay ignored; nothing new reaches picorv32 inputs from a
  non-ignored test.

## 1. Launch the picorv32 gate detached — first

```
cargo test --release -p grus-core --test drt \
    test_emit_selfdrc_picorv32 -- --ignored --nocapture \
    2>&1 | tee /tmp/emit-picorv32.log
```

Run detached (nohup / separate shell), budget ≈ route (~135 min)
+ emit + parse + `run_all` + byte-check writes. Captures for
`results.md`: routed/unrouted/wire_rects/vias line, every unrouted
name, emit census block (wirelength + per-cut via deltas), gate
findings (or the empty `firing` set), and wall clock.

## 2. In-suite gcd legs

```
cargo test --release -p grus-core --test drt emit -- --nocapture \
    2>&1 | tee /tmp/emit-gcd.log
```

Runs the six non-ignored emit tests (gcd routes ≈ 2 s release).
Capture the census + `self_drc findings_total=0` lines verbatim.

**If `run_all` fires on gcd**: classify per selfdrc.md's table —
fix in `maze.rs`/`emit.rs` in place when the root cause is small
and clear; otherwise name rule + count + class in `results.md`.
Never loosen the assert, never skip the leg.

## 3. CLI end-to-end on gcd (release binary)

```
cargo run --release -p grus-eda -- drt \
    --tlef ~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef \
    --lef  ~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef \
    --def  tests/designs/gcd/gcd.placed.def \
    --guide tests/designs/gcd/gcd.guide \
    --out-def /tmp/gcd.drt.ours.routed.def \
    --reference tests/designs/gcd/gcd.drt.reference.json \
    --self-drc
```

Proves the delegation-replacement surface end to end; its census
block joins the test-side numbers in `results.md`. Output DEF is a
`/tmp` artifact — not committed (`e6-drt-e2e` owns the committed
ours-DEF).

## 4. Docs (docs.md), drafted while step 1 runs

`docs/e6-drt.md` §8, `docs/results.md` dated entry with **measured**
numbers only (gcd from step 2–3 captures; picorv32 fills in when
step 1 lands — mark the entry's picorv32 row only after the log
exists), `docs/status.md` head line, `docs/roadmap.md` bullet.

## 5. Version + verify

- `[workspace.package] version` `0.12.4 → 0.12.5` (feature → patch
  on 0.x), same commit.
- `cargo test --workspace` — must complete inside the verify
  budget; the default set carries only gcd-scale emit legs.
- `cargo clippy --workspace --all-targets -- -D warnings`.

## 6. Commit

Conventional Commit, English, agent trailer:

```
feat: Routed DEF emission and self-DRC gate
```

Body bullets: emit model (no-width grammar, NETS splice, square-cap
model, ViaTable-default vias), `grus-eda drt` surface, the maze-side
occupancy/stub fixes with their motivation (self-DRC findings),
test list, verdict + census headline, remaining (`e6-drt-e2e`).

## Contingencies

- **gcd gate red** — see step 2. A spacing red reachable by the
  `Disabled` arm but not `Enabled` means the gate is vacuous —
  investigate the fixture, don't accept the pass.
- **picorv32 leg still running at verify time** — the kurena verify
  (`cargo test --workspace`) never runs it; the results entry waits
  for the log. If it cannot finish inside the ticket's wall clock,
  commit the gcd-complete entry with the picorv32 leg recorded as
  "running detached — command + start time" and land the numbers in
  a follow-up amend only if the card is still open; otherwise the
  e2e ticket inherits it. Do **not** gate-verify an unfinished
  claim.
- **picorv32 unrouted count drifts from the pinned 108** — expected
  direction is *fewer* `NoPath` (io-port/PDN marking changes the
  pricing, not the graph). Record the fresh count + class split and
  note the premark change as cause; a *larger* or reclassifying
  residual is a maze regression — investigate before naming.
- **Byte-check red on re-emit** — grammar drift on the second pass
  (foreign-geometry arm of `centerline`): a `wire_rect` the parser
  produced that isn't square-cap-expressible. Debug against
  `parse_net_wire` token-by-token; the via-record line is the
  fragile spot.
- **Pinned maze test red** from the occupancy changes — per
  tests.md: investigate the shifted number, fix or record with
  cause; do not re-pin silently.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
