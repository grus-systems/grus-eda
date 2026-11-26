# e7-drc-e2e: `grus-eda drc` CLI + full subset gate

E7 ticket 5 (stage gate), depends on `e7-drc-enc-area`. Wrap the checker
in a CLI and gate all 50 unique `lef_expressible` rules against the
committed `magic_per_rule` surface on both routed DEFs.

## Scope

1. **`grus-eda drc`** — args mirror `irdrop`/`power`: `--tlef`, `--lef`,
   `--def` (routed), optional `--reference <d>.drc.reference.json` to
   print per-rule deltas, `--out`/`--json` report. Output: per-rule
   violation counts + violation bboxes (bbox list format compatible
   with a later `drc listall` comparison), plus a summary JSON.
2. **Integration gate** (`crates/grus-core/tests/drc.rs` or similar):
   load both routed DEFs, run all implemented rules, assert every
   `lef_expressible` rule reports exactly the `magic_per_rule` count —
   expected all-zero today — and that the checker's rule-name set covers
   every `lef_expressible` name in `magic_rules_checked.classification`.
3. **Honest reporting**: print the deck split (55 lef_expressible
   statements implemented of 278; the 223 magic_only are out of scope by
   the taxonomy) so the CLI output names its claim boundary.
4. **Mutation parity**: a test mutates a copy of the routed DEF the way
   `generate_drc_ref.sh`'s sanity control does (met2 wire at 0.03 µm)
   and asserts our `met2.2` fires — matching the Magic result on the
   same input shape.

## Files

- `crates/grus-eda/src/main.rs` (`drc` subcommand)
- `crates/grus-core/src/drc/report.rs` (JSON report + bbox emission)
- `crates/grus-core/tests/drc.rs` (gate)
- `docs/e7-drc.md` (CLI + claim-boundary section)
- `docs/results.md` (dated entry — gate results, runtime)
- `docs/status.md` (E7 done-with-named-residual line)
- `docs/roadmap.md` (E7 progress line)
- `tests/designs/README.md` if new fixtures land
- `Cargo.toml`/`Cargo.lock` (version bump — CLI surface lands minor)
