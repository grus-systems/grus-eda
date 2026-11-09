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
- Record every comparison in `docs/results.md`: date, design, reference tool value, our value, difference, and our runtime.
  Keep failed or divergent runs in the log as well, along with the root cause identified.
- Commit reference tool output files (golden files) only when they are small (< 1 MB). For larger outputs, commit the exact reproduction script/command instead.
- Reference tools come from the cached LibreLane container image (`ghcr.io/librelane/librelane:3.0.16`).
  Start Docker via Colima:
  `mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8`
  Invoke OpenSTA and OpenROAD inside the container as `sta` and `openroad`.
- Speed optimizations (parallelism via `rayon`, SIMD, `Accelerate`, Metal GPU) come **only after** a stage's simple CPU version agrees with the reference tool. Lock in equivalence between the fast path and the simple baseline with regression tests.
- Always state explicitly what you actually ran vs. what is untested. Never claim something "should work" based on guesswork.

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

## 6. First Steps (E0)

1. Create the Cargo workspace (`grus-eda` CLI binary + `grus-core` library crate, following `docs/architecture.md`).
2. Add `gcd` (`sky130hd` from OpenROAD-flow-scripts) under `tests/designs/gcd/` and document how to generate the Yosys netlist (`.json` and `.v`) and OpenROAD placed `.def` using the LibreLane container in `tests/designs/README.md`.
3. Implement the Liberty reader and binary cache first (`docs/e0-readers.md`), and compare cell count, pin count, and NLDM table dimensions against OpenSTA (`get_lib_cells`, `report_lib_cell`) on the same `.lib` file.
