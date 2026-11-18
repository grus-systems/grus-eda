# grus-eda — AI Agent Guide

Instructions for AI coding agents working in this repository.
**This file is self-contained.** Do not read instruction files outside this repository.

## 1. Hard Rules

- **Only public designs and code written in this repository may enter this repository.**
  - Test designs: OpenROAD-flow-scripts `sky130hd` designs (`gcd`, `aes`, `ibex`, etc.) and `picorv32`.
    Before adding a design, verify its open-source license and document its provenance in `tests/designs/README.md`.
  - PDK: SkyWater sky130 (`sky130_fd_sc_hd`).
  - Even if the user hands you a design file or netlist, confirm it is public before adding it; if unsure, ask first.
- Never use `--force`, `--no-verify`, `reset --hard`, `branch -D`, or `push --force` without explicit permission from the user.
- Ask before installing new system tools or running any billable operation. Adding Rust crates via `Cargo.toml` does not require asking first (explain the reason in the commit message).

## 2. Verification Discipline (Most Important Rule)

- Every stage (`E0` through `E3` in `README.md`) has a **reference tool**. **Passing your own unit tests does not mean a stage is done.** A stage is done only when run on the exact same inputs as the reference tool and its numbers meet the "done when" criteria in `README.md`.
- **Never hand-write or round reference numbers**: Golden reference files (`*.reference.json`) must be emitted directly by Tcl/scripts running inside OpenROAD or OpenSTA, and integration tests must deserialize and assert against those files directly.
- **No silent test skips**: Integration tests that require PDK or design files must fail loudly if the files are missing (do not `eprintln!("Skipping...")` and `return` green).
- **Honest runtime measurement**: When reporting cold parse vs. warm cache runtimes in `docs/results.md`, explicitly remove the cache file before timing the cold path.
- Record every comparison in `docs/results.md`: date, design, reference tool value, our value, difference, and our runtime.
  Keep failed or divergent runs in the log as well, along with the root cause identified.
- Commit reference tool output files (golden files) only when they are small (< 1 MB). For larger outputs, commit the exact reproduction script/command instead.
- Reference tools come from the cached LibreLane container image (`ghcr.io/librelane/librelane:3.0.16`).
  Start Docker via Colima:
  `mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8`
  Invoke OpenSTA and OpenROAD inside the container as `sta` and `openroad`.
- Speed optimizations (parallelism via `rayon`, SIMD, `Accelerate`, Metal GPU) come **only after** a stage's simple CPU version agrees with the reference tool. Lock in equivalence between the fast path and the simple baseline with regression tests.
- Always state explicitly what you actually ran vs. what is untested. Never claim something "should work" based on guesswork.

## 3. Picking Up Work

- Status, tolerances and open tasks for E3 are in `docs/e3-sta.md` Sections 4-6 (open tasks `E3.4`-`E3.6`, each with
  why, reference, approach and done criterion). `docs/results.md` is the log of every comparison, including failures.
- Before running tests that use `picorv32`, generate its inputs (`docs/e3-sta.md` Section 6).
- Licensing: OpenROAD is BSD 3-Clause; since the user's decision of 2026-11-15 its source may be read (the
  pinned revision is the reference), but its code must not be copied into this repository — implementations
  here are our own. OpenSTA is GPL: its source must not be read or copied; use the published papers and the
  tool's output instead. Check the license of any third-party code or table before adding it.

## 4. Working Style & Communication

- **Chat responses to the user must be in concise Japanese.**
- **All repository text—code comments, identifiers, commit messages, `README.md`, `AGENTS.md`, and `docs/`—must be in English.**
- Keep technical terms in their standard form; add brief parenthetical notes in chat responses when helpful.
- Ask early when requirements are ambiguous. Make minor technical choices yourself and record the decision and rationale in the commit message or `docs/`.
- Keep changes small and focused; do not mix unrelated concerns in a single commit.
- Write code comments only to explain *why*, not *what*. Handle errors at system boundaries (file I/O, CLI parsing), not inside hot loops.
- Leave no half-baked backward-compatibility shims or unused `_var` bindings.

## 5. Git & Commits

- **Commit proactively**: As soon as a logical unit of work (a doc update, a module, a parser sub-step, or a verification run) is complete and passes checks, commit it immediately without waiting to be asked.
- Work on `dev` or a feature branch cut from `dev` and merge via PR into `dev` (`dev` $\rightarrow$ `main` is decided by the user).
- Use Conventional Commits in English (`feat:`, `fix:`, `docs:`, `test:`, `chore:`, `refactor:`, with `!` for breaking changes).
- Keep the configured git author identity (`Yuto Ohashi <yuto@grus.systems>`) unchanged.
- Multiple agents may work concurrently. When working on a branch other than `dev`, create a separate worktree via `git worktree add ../grus-eda-<topic> <branch>`. Never run `git add -A`; always stage explicit file paths.
- Versioning: Once the Cargo workspace exists, bump `[workspace.package] version` in the root `Cargo.toml` in the same commit as any functional change. While in `0.x`, features and bug fixes bump `patch`; breaking changes bump `minor`.
- Append your agent trailer to every commit message and PR description (e.g., `Co-Authored-By: <Agent Name> <email>`).

## 6. Kurena Workflow

- Implementation tasks may be driven through kurena (`kurena` on PATH): write `kurena/tasks/<slug>/spec/{goal.md,acceptance.md}` — goal lists intent plus the files to touch, acceptance is a checklist — then `kurena push <slug>` runs plan → implement → verify → review in `.kurena/wt/<slug>`.
- `workflow.toml` configures the drivers (devin ACP) and verify commands (`cargo test --workspace`, clippy `-D warnings`). Do not hand-edit `.kurena/`; spec and design docs under `kurena/tasks/` are pipeline artifacts and stay committed.
- Cards are human-gated: present `kurena show <slug>` output and let the user run `approve` / `reject <memo>` / `edit` / `abort`. Never run these yourself.

## 7. Lessons from the E0 Review (2026-11-09)

An outside review of E0 found that the stage did pass, but parts of the record could not be
reproduced. Each item below is a rule that came out of that review.

1. **A reference value must come from the committed script, run as written.**
   `gcd.reference.json` once held numbers typed into a Python heredoc. Later, the committed value
   `bterms_signal = 54` could not be reproduced: run as written, `generate_ref.sh` gave 1. Before
   committing a golden file, rerun the exact script that is committed, then `git diff` the output.
2. **Never derive a reference value by formula or from a different input.**
   `def_tracks_statements = layers * 2` and Verilog pin counts taken from the placed DEF both
   happened to be right, but neither measured anything. Ask the tool for the exact quantity, from
   the exact file the test reads.
3. **Tcl fed to `openroad` / `sta` on stdin: `!` is history expansion.** `if {![$x isSpecial]}`
   silently breaks there. Write `[...] == 0`.
4. **Claim coverage only for what an input exercises.** "Matches under all 8 orientations" was
   written while gcd only uses N/S/FN/FS, and W/E/FW/FE were in fact swapped. When a design does
   not cover a case, build a fixture that does (`gcd.orient.def`) before claiming it.
5. **For integer quantities, a small difference is a bug, not a tolerance.** The HPWL gap of 5 DBU
   (0.0001%) hid a systematic 1-DBU error on 208 of 877 pins: a rounding-order mismatch with
   OpenROAD's `getAvgXY`. Compare per element (per pin, per net), not only the total. Loosen a
   tolerance only after the root cause is named in `docs/results.md`.
6. **Reproduce the reference tool's arithmetic, not just its formula.** That means the order of
   operations, the rounding direction (`int(double)` rounds toward zero), and when a transform is
   applied. Write the rule in `docs/` first, then the code.
7. **Prove that a test can fail.** After a fix, break it on purpose (for example, disable the
   rounding correction) and confirm the test goes red. Then restore it.
8. **Keep `docs/` in step with the code.** `e0-readers.md` described a bbox-union pin center
   that the code never implemented. If the code is right, fix the spec in the same change.
9. **Say what you ran.** If you rerun only part of a script (for example, steps 6–8 of
   `generate_ref.sh`), name the steps in the commit and in `docs/results.md`.
10. **Time a cold path only after deleting the cache.** A "cold parse 5 ms" figure was really a
    cache hit (the real cold parse is about 50 ms).
