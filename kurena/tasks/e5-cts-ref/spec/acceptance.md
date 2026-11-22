# Acceptance

- [ ] Committed script runs TritonCTS +
      `repair_clock_nets` + placement parasitics +
      the skew/cts report surface on `gcd.placed.def`
      inside the pinned image
      (`ghcr.io/librelane/librelane:3.0.16`); re-running
      it byte-reproduces the committed artifacts.
- [ ] `gcd.cts.def`, `gcd.cts.reference.json` and
      `gcd.cts.sta.reference.json` committed under
      `tests/designs/gcd/`; JSON values emitted by the
      Tcl/script itself, never hand-written; sizes
      recorded.
- [ ] `docs/results.md` entry: script steps,
      measured skew/insertion-delay/buffer-census/
      wirelength values vs the P-CTS scoping numbers
      (5 `clkbuf_16` + 3 dummies, 35 sinks, depth 2,
      ≈0.31 ns, ≈0 ps skew) — confirmed or corrected.
- [ ] picorv32 CTS reference included or explicitly
      deferred to the next ticket with reason.
- [ ] `cargo test`, `clippy -D warnings` pass;
      version bumped only if Cargo-visible behavior
      changes (reference-only ticket may skip the bump
      per convention if no code moves — state which).
