# Ticket 02 (F9 Part A): test_support module, Liberty cache relocation, bench

## Goal

Implement Part A of `docs/design/f9-test-support-ci.md` including the
review resolutions at its end (external review):

1. `crates/grus-core/src/test_support.rs` (`#[cfg(test)]`, registered in
   `lib.rs`): `pdk_root()` ($PDK_ROOT else $HOME/.ciel, panicking
   clearly when neither is set), `sky130_hd()`, `design_dir(name)`
   (= `CARGO_MANIFEST_DIR/../../tests/designs/<name>`), and
   `require(path)` (panics with the full path when missing). Migrate
   every test module to it: all local `dirs_home()` copies and all
   open-coded PDK_ROOT/HOME path chains in test code are deleted. No
   test's assertions or failure behaviour change (missing files still
   panic loudly).
2. Liberty cache relocation in `crates/grus-core/src/readers/liberty.rs`:
   the only cache location is the cache directory —
   `$XDG_CACHE_HOME/grus-eda/` (ignored when empty or relative), else
   `$HOME/.cache/grus-eda/`, else `temp_dir()/grus-eda-cache-<USER>/`
   (`<USER>` = $USER when set, else `shared`) — file name keyed by the
   source xxh3 as the current fallback does. The next-to-the-`.lib`
   path is neither read nor written. `CacheFile` gains a `version`
   field (value 1), checked on load; mismatch is a miss. Writes use a
   unique temp name in the same directory (process id + atomic
   counter) and rename into place.
3. The wall-clock assertion block in `test_sky130_liberty_integration`
   is deleted (value assertions stay). New bench
   `crates/grus-core/benches/liberty_cache.rs` (`[[bench]]`,
   `harness = false`, std only): resolves the sky130 tt liberty file
   with its own small lookup (comment pointing at `test_support` as
   canonical), prints cold-parse and warm-cache load times, asserts
   nothing, exits non-zero with a clear message when the PDK is absent.

## Files

- `crates/grus-core/src/test_support.rs` (new), `crates/grus-core/src/lib.rs`
- Test modules across `crates/grus-core/src/` (path lookups only)
- `crates/grus-core/src/readers/liberty.rs` (cache + the one test edit)
- `crates/grus-core/benches/liberty_cache.rs` (new),
  `crates/grus-core/Cargo.toml` (bench registration)
- `docs/results.md` (F9 Part A entry)
- `Cargo.toml` / `Cargo.lock` (version bump 0.1.24 → 0.1.25)

## Acceptance

Run by you (no PDK on this machine):

- [ ] `grep -rn "fn dirs_home" crates/` returns nothing; `grep -rn
      "PDK_ROOT" crates/grus-core/src/` appears only in `test_support.rs`
      and the bench.
- [ ] PDK-free tests pass; PDK-dependent tests fail with the loud
      missing-path panic (spot-check two and quote the messages).
- [ ] A unit test for the cache directory resolution (env combinations:
      XDG set/unset/relative, HOME unset) and one proving a written
      cache lands under the cache directory and not next to a `.lib`
      in a temp source dir. Prove they can fail (e.g. restore the
      next-to-lib primary, see red, restore) and say so.
- [ ] `cargo clippy --all-targets -- -D warnings` and
      `cargo build --release` pass.

Run by the maintainer on the Mac mini:

- Full `cargo test --release` green; the liberty integration test no
  longer contains a timing assertion (grep).
- `cargo bench --bench liberty_cache` prints cold and warm timings.
- No `.lib.grus.cache` file appears next to the PDK liberty after a
  test run; a cache entry appears under the cache directory.

## Notes

- Part B (the CI workflow) is a separate ticket; do not add `.github/`.
- Do not commit; the maintainer commits after the mini acceptance run.
