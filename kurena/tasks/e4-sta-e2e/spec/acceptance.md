# Acceptance

- [ ] OpenSTA endpoint reference on the `read_guides`
      parasitics path committed via script (pinned
      image); our guide-SPEF → `sta` compared per
      endpoint (arrival/required/slack).
- [ ] gcd `max(5 ps, 1%)` evaluated with actual numbers;
      picorv32 evaluated against its status.md floors;
      any outside-bound endpoint root-caused (guide
      topology vs parasitics vs warned-pin residual —
      no unexplained loosening).
- [ ] `docs/results.md` entry with probe conditions and
      per-endpoint numbers; E4 section status updated.
- [ ] `cargo test`, `clippy -D warnings`, release sta
      regression pass; existing goldens byte-identical;
      version bumped.
