# Acceptance — drt-unrouted-census

- [ ] `== drt emit:` census prints unrouted pins broken down per
      `FailKind`.
- [ ] Each `  unrouted <net>` line carries per-pin failure kinds.
- [ ] `region_escape` provenance is visible in the output where it
      applies (not collapsed silently).
- [ ] `route_db` digest unchanged (print path only).
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/results.md` dated entry with sample output.
