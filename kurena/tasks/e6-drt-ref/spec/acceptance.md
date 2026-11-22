# e6-drt-ref — acceptance

- [ ] `generate_drt_ref.sh` runs both variants (baseline + delegated)
      inside the pinned image for gcd and picorv32; re-running
      byte-reproduces the JSON (or names the metadata field that
      varies)
- [ ] `gcd.drt.reference.json` + `picorv32.drt.reference.json`
      committed: DRC counts, antenna counts, wirelength, per-layer via
      counts, routed-DEF SHA-256 prefix + size, runtime — all
      script-emitted
- [ ] Delegated run (`read_guides *.guide.ours`) result recorded
      verbatim — including if it produces nonzero DRC or fails (that
      is data, not a blocker)
- [ ] Routed DEFs gitignored with SHA/size in JSON; DRC/antenna
      reports committed if <1MB
- [ ] `docs/results.md`: commands verbatim, baseline-vs-delegated
      delta table, rerun count, any deviations from scoping probes
- [ ] `cargo test --workspace` + `clippy -D warnings` still pass;
      no version bump (reference-only) stated in results.md
