# Acceptance

- [ ] `docs/results.md` records the measured OpenSTA SAIF
      semantics (field→activity mapping, SAIF-vs-VCD agreement on
      the same window, hierarchy-resolution behaviour) — from
      runs inside the LibreLane container.
- [ ] `grus-eda power --saif tests/designs/microbench/mb.saif`
      produces a report matching the OpenSTA-emitted golden at
      STRICT tolerance; unmatched SAIF scopes are counted in
      diagnostics.
- [ ] The golden values come from running the committed Tcl
      inside `ghcr.io/librelane/librelane:3.0.16`, not hand-derived.
- [ ] `cargo test --workspace` and `cargo clippy --workspace
      --all-targets -- -D warnings` pass; VCD path unchanged
      (existing golden tests still pass).
- [ ] `docs/roadmap.md` N7 updated; version bumped in the same
      commit.
