# e8-e2e — Implementation order

One focused sequence. The comparator/test, generated feed publication,
documentation, and any named-floor correction belong together because
the acceptance verdict depends on all of them.

## 1. Publish the reference per-instance feed

Update `tests/designs/generate_irdrop_ref.sh` step D so it copies
`runs/irdrop/inst_power.tcl` to `<d>.irdrop.inst_power.tcl` and adds
`feed_file`, `feed_size_bytes`, and `feed_sha256_16` to
`inst_power_feed` in `<d>.irdrop.reference.json`.

Rerun inside the pinned container:

```bash
bash tests/designs/generate_irdrop_ref.sh gcd
bash tests/designs/generate_irdrop_ref.sh picorv32
```

Then rerun at least once more per the committed-script rule and `git
diff` the artifacts. The new Tcl files and updated reference JSONs are
the only expected changes; any unrelated golden drift stops the task.

## 2. Build the integration-test harness

Create `crates/grus-core/tests/irdrop.rs` with the helpers in
`harness.md`:

- local PDK/design lookup and loud `require`;
- design/solve loader;
- reference JSON scalar reader;
- six-column CSV parser;
- `(Instance, Terminal)` set assertion;
- `inst_power.tcl` parser and `CellId` mapping;
- deviation/top-5/statistics helpers.

Keep it test-local. Do not add a `pdn::compare` module solely to serve
the test.

## 3. Main e2e gate

Implement `test_irdrop_matches_reference`:

- recompute both nets on both designs;
- byte-check the committed ours CSVs against `write_voltage_csv`;
- assert identical row sets before values;
- print all metrics and top-5 pins;
- assert scalar worst-drop/bounce bounds on all nets;
- assert strict per-pin bounds on gcd and picorv32 VPWR;
- bound picorv32 VGND with the named `0.0032 V` floor until the axis
  result justifies a different number.

Use one accumulating gate function so picorv32 still prints when gcd
fails.

## 4. Reference-power decomposition

Implement `test_irdrop_reference_power_axis` on both designs:

- assert feed coverage and `inst_feed_total_w`;
- re-solve each net using the feed;
- print `estimator`, `mesh`, and `total` axes;
- use the axis results to name the picorv32 residual and, if needed,
  any gcd residual.

Do not set an unmeasured axis tolerance; the diagnostic records the
numbers first.

## 5. Fail-proof check

Run the planned VGND `.sta.csv` substitution break on gcd. Confirm the
gate goes red on per-pin voltage deviations, restore the file/line,
and rerun green. Record the exact mutation and output.

## 6. Documentation

Update in this order:

1. `tests/designs/README.md` — new feed artifact provenance.
2. `docs/results.md` — full gate, decomposition, artifacts, mutation,
   commands, and what was run.
3. `docs/status.md` — correct the E8 verdict and the previous
   per-pin overclaim.
4. `docs/roadmap.md` — E8 gate landed, done or named residual.

Keep `docs/results.md` chronological and do not revise the earlier
`e8-solve` entry.

## 7. Final verification

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p grus-core --test irdrop -- --nocapture
```

Also verify the committed ours CSVs against fresh `grus-eda irdrop`
runs only if implementation changes production behavior; otherwise the
in-test byte comparison is sufficient and should be stated as such in
`docs/results.md`.

## Acceptance map

| Acceptance item | Where handled |
|---|---|
| gcd both nets satisfy both bounds, or a miss is decomposed and named | `test_irdrop_matches_reference`; `test_irdrop_reference_power_axis`; `decomposition.md` |
| picorv32 both nets measured/recorded; floor only with named cause | picorv32 policy; axis test; `docs/results.md` |
| identical `(Instance, Terminal)` set before values | CSV row-set assertion in `harness.md` |
| VGND as bounce, absolute-voltage comparison, sign convention written down | comparator formulas; `docs/results.md` sentence |
| top-5 deviation pins per net | `MapStats.top5` print and results table |
| workspace tests/clippy; version decision | steps 7; `docs.md` version rule |
| results/status/roadmap updated; E8 done or residual named | step 6 |

## Risks

- **Stale `picorv32.placed.def`**: the gitignored input must be
  regenerated before tests; the panic must name
  `tests/designs/picorv32/generate_ref.sh`.
- **Feed provenance**: additive annotation semantics mean the feed is
  evidence, not a literal embedded-state dump; always carry the
  `inst_feed_total_w` vs `embedded.total_power_w` distinction.
- **VGND interpretation**: the strict bound is relative to the
  absolute golden voltage, so VGND's near-zero map uses the `1 mV`
  floor. Do not reinterpret `5%` as 5% of the `1.8 V` rail.
- **Artifact drift**: fresh solves are compared byte-for-byte to the
  committed ours CSVs; if a production change alters numeric output,
  regenerate and commit the maps rather than weakening the check.

## Files

- `crates/grus-core/tests/irdrop.rs`
- `tests/designs/generate_irdrop_ref.sh`
- `tests/designs/gcd/gcd.irdrop.inst_power.tcl`
- `tests/designs/gcd/gcd.irdrop.reference.json`
- `tests/designs/picorv32/picorv32.irdrop.inst_power.tcl`
- `tests/designs/picorv32/picorv32.irdrop.reference.json`
- `tests/designs/README.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
