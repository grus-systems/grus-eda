# e7-drc-ref — acceptance

- [ ] `generate_drc_ref.sh` runs inside the pinned image and emits
      `gcd.drc.reference.json` (+ picorv32 if input routed DEF
      exists): Magic total + per-rule counts, version, DRT report
      count, antenna count — all script-emitted
- [ ] Magic per-rule breakdown present (`drc listall why` or
      equivalent), not just the total
- [ ] Sanity control: scripted broken DEF (named mutation, e.g.
      shifted wire) produces nonzero Magic DRC and names the violated
      rule — evidence the surface detects violations
- [ ] Rule-category taxonomy committed (LEF-expressible vs antenna vs
      Magic-only) — this bounds any future `grus-eda drc` claim
- [ ] Rerun byte-reproduces the JSON (or varying metadata named)
- [ ] `docs/results.md`: commands verbatim, counts, taxonomy summary,
      runtime; `cargo test --workspace` + `clippy -D warnings` pass;
      no version bump (reference-only) stated
