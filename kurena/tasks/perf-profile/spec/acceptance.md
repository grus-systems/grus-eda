# Acceptance

- [ ] Phase timing available for the heavy commands'
      natural boundaries (flag-gated or stderr); reader
      `elapsed_ms` lines unchanged.
- [ ] `docs/results.md` entry: per-design wall-clock table
      (ibex/aes/jpeg/picorv32/gcd, release build, machine
      context + rep policy recorded), per-phase breakdown,
      ranked hotspot list, peak-RSS note.
- [ ] Optimization plan committed: dominant phases named,
      parallelizable vs serial/memory-bound classified,
      recommended ticket order with Amdahl ceilings; rayon
      plug-in points identified.
- [ ] No functional change to outputs — goldens byte-
      identical; `cargo test --workspace` + clippy pass;
      version bumped only if code lands.
