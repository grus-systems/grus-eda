# Acceptance

- [ ] OpenSTA's un-annotated default activity (input rate,
      sequential default) measured and recorded verbatim.
- [ ] Combinational and sequential propagation rules
      measured on discriminating fixtures (incl. a toggling/
      counter register for divide-by-2).
- [ ] Partial-annotation fallback semantics recorded (which
      nets propagate vs stay zero).
- [ ] Either implementation lands with golden coverage, or a
      sequenced ticket plan is committed in docs.
- [ ] `cargo test --workspace` + clippy pass; existing
      goldens byte-identical; version bumped.
