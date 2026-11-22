# perf-par-slew — verification

Equivalence must be *recorded*, not asserted (`AGENTS.md` §2, E0-review
lessons 4 and 7: prove the test can fail; claim coverage only for what an
input exercises). Three evidence layers plus the fail-proof run; all four
land in the `docs/results.md` entry and the commit message.

## 1. Existing strict goldens (the equivalence proof)

- `cargo test --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --release -p grus-core sta` (183 tests, 2
  ignored at v0.3.2) — all green. The strict `*.reference.json` endpoint
  comparisons exercise `propagate_slew` through `prepare_sta` on every
  committed design and clock mode (ideal, `set_propagated_clock`, pulse
  width); `power` paths hit it via `sta_pin_slews`.
- `git status` under `tests/designs/**` must stay clean — no golden file
  changes. Byte-identical requirement means *no* tolerance adjustment is
  acceptable anywhere.
- Unit-level coverage: `test_propagated_clock_gate_merges_enable_arc_slew`
  calls `propagate_slew` directly and asserts the enable-arc merge wins —
  it goes red if the merge is perturbed.

## 2. Serial-vs-parallel output diff (direct bit-identity)

Before touching `analysis.rs`, capture baseline reports with the
*pre-change* release binary into `/tmp` (keep them for the cmp):

```sh
cargo build --release
for d in picorv32 aes ibex jpeg; do
  target/release/grus-eda sta --lib "$LIB" --tlef "$TLEF" --lef "$LEF" \
    --def tests/designs/$d/$d.placed.def --sdc tests/designs/$d/$d.sdc \
    --parasitics placement --json /tmp/$d.before.json
  # and the SPEF mode for the designs that have routed.spef
done
```

After the change, rerun the same commands to `/tmp/$d.after.json` and
`cmp` each pair — must be byte-identical. (`--json` serializes `f32` via
serde's shortest round-trip, so identical bits ⇒ identical bytes.) Record
which designs/modes were compared.

## 3. Determinism under thread-count sweep

On jpeg `sta --parasitics placement` (largest level structure):

```sh
for t in 1 4 10; do
  RAYON_NUM_THREADS=$t target/release/grus-eda sta ... --json /tmp/jpeg.t$t.json
done
cmp /tmp/jpeg.t1.json /tmp/jpeg.t4.json && cmp /tmp/jpeg.t4.json /tmp/jpeg.t10.json
```

`RAYON_NUM_THREADS=1` runs the `par_iter` bodies on one thread — the same
apply-after-collect discipline with zero scheduling freedom. Byte-identity
across thread counts + the disjoint-write/`BTreeMap`-lookup-only
construction argument ([`algorithm.md`](algorithm.md)) is the recorded
determinism proof. Repeat on ibex — it takes the `include_seq=false`
fallback, the mode where the seq fixup exists for.

## 4. Fail-proof run

Record an intentional break, the tests it sends red, and the restore:

- Suggested break: in the apply step skip writing `slew_min` (or swap
  `merge_slew`'s `max`/`min`). Expected red:
  `test_propagated_clock_gate_merges_enable_arc_slew` plus golden
  comparisons in `cargo test -p grus-core sta`. Record actual test
  names/counts in the commit message and `docs/results.md`.
- Also record the fixup-coverage probe honestly: run the suite (or the
  release `sta` runs) once with a temporary counter/`eprintln!` on the
  fixup branch and report whether any committed design exercises a
  same-level-earlier seq edge. If none does, say the fixup is
  defensive-but-required (it preserves semantics the code cannot prove
  absent) and name it — do not claim coverage the inputs do not exercise.
  If ibex does exercise it, the ibex goldens are its correctness proof.

## Notes

- No new committed test is spec'd: equivalence is locked by the existing
  strict goldens + recorded cmp/sweep runs, the same discipline
  `eval_sta`'s `par_iter` walk already operates under.
- `GRUS_PHASE_TIMING` output must still contain
  `sta.prepare.clock.slew` — grep the gated stderr of one measured run
  and record it.
- If the fixup probe shows ibex (or any design) fires it frequently and
  the serial fixup share shows up in the phase number, name it in the
  residual analysis rather than hiding it in "serial tail".
