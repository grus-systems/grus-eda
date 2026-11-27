# Acceptance — drt-thread-param

- [ ] No test mutates the process environment: `set_var`/`remove_var`
      gone from `crates/grus-eda-core/tests/drt.rs`,
      `with_rayon_threads` and `THREAD_ENV_LOCK` deleted.
- [ ] Thread count reaches every rayon pool construction as an
      explicit input; unset/default preserves current behavior.
- [ ] The former forced-thread arms still pass at both thread counts
      with explicit parameters — same assertions, same digest pins.
- [ ] Routed output byte-identical: DEF golden tests and digest pins
      unchanged.
- [ ] `cargo test --workspace --profile verify` and clippy
      `-D warnings` pass.
- [ ] `docs/results.md` dated entry recording the API change and the
      evidence above.
