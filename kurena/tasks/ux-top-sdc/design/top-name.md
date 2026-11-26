# `--top` matching design

## Scope

`--top` exists only on `check-e0` and `floorplan`. Both commands load a design name and pass it to `check_top_name`:

- `check-e0`: Yosys JSON, Verilog, and DEF each run the same check.
- `floorplan`: the selected Yosys JSON or Verilog design runs the same check.

Keep one shared helper so all paths have identical rules.

## Matching rule

Add a tiny pure helper near `check_top_name`:

```rust
fn top_name_matches(requested: &str, actual: &str) -> bool {
    !requested.is_empty()
        && (requested == actual || actual.rsplit('\\').next() == Some(requested))
}
```

Then keep `check_top_name` as the boundary that turns a mismatch into the existing loud error:

```rust
fn check_top_name(requested: Option<&str>, kind: &str, actual: &str) -> anyhow::Result<()> {
    if let Some(top) = requested
        && !top_name_matches(top, actual)
    {
        anyhow::bail!("{kind} design name {actual:?} does not match --top {top:?}");
    }
    Ok(())
}
```

Rules:

- `None` is always accepted; callers continue to rely on reader auto-selection.
- `Some(name)` is accepted when `name == actual`.
- Otherwise it is accepted only when `name` equals the non-empty segment after the last literal `\` in `actual`.
- Only the last `\` matters. For `$paramod$abc\sub\picorv32`, `picorv32` is accepted and `sub` is rejected.
- `corv32`, `picorv`, `$paramod$abc`, `paramod`, and other substring/prefix candidates are rejected.
- An empty `--top ""` is rejected rather than matching a trailing backslash.
- The comparison is on the loaded `actual` string (`DesignDb.design_name` / equivalent), not raw file spelling. This keeps DEF escaping behavior out of the CLI rule.

The existing error string can remain `{kind} design name {actual:?} does not match --top {top:?}`; it already names the loaded and requested values, and the new help text explains accepted forms.

## Help text

Use the same wording for both `CheckE0Args::top` and `FloorplanArgs::top`:

```rust
#[arg(
    long,
    help = "Top-level design name (exact name or final hierarchy segment after `\\`; omit to auto-select)"
)]
top: Option<String>,
```

If Clap help renders the backtick/backslash text awkwardly, use equivalent plain text but preserve all three facts: exact name, final segment after `\`, and omission auto-selects.

## Unit tests

Add one focused pure test in the existing `#[cfg(test)]` module in `main.rs`. It must not load the sky130 PDK.

Suggested cases:

```rust
#[test]
fn check_top_name_matching_rules() {
    let actual = "$paramod$abc\\sub\\picorv32";

    assert!(check_top_name(Some(actual), "netlist", actual).is_ok());
    assert!(check_top_name(Some("picorv32"), "netlist", actual).is_ok());
    assert!(check_top_name(None, "netlist", actual).is_ok());

    for top in ["sub", "$paramod$abc", "corv32", "picorv", "picorv32x", ""] {
        let err = check_top_name(Some(top), "netlist", actual).unwrap_err();
        assert!(err.to_string().contains("does not match --top"));
    }
}
```

This covers the four requested cases directly:

| Case | Input | Expected |
|---|---|---|
| Exact name | `Some(actual)` | accepted |
| Final segment | `Some("picorv32")` | accepted |
| Wrong/substring/prefix | `corv32`, `picorv`, `picorv32x`, `$paramod$abc` | rejected |
| Non-final collision | `sub` | rejected |
| Omitted top | `None` | accepted |

Use `rsplit('\\').next()`, not a generic `contains`/`ends_with`/`split_last` shortcut, so the implementation mirrors the documented "final hierarchy segment" contract.

## Single-top auto-accept verification

Code inspection already establishes the pre-change behavior:

- `parse_yosys_json_bytes` selects the first non-blackbox module with cells, falling back to the first module.
- The Verilog reader uses the first `module` declaration.
- `check_top_name(None, ...)` returns `Ok`.

In the existing `cli_end_to_end_gcd_flow_and_validation` test, change the successful `check-e0` invocation to omit `--top`. It already passes JSON, Verilog, and DEF for `gcd`, so it proves the auto-accept path across all three readers. If adding one command does not make the test materially larger, also run `floorplan --json ... --out <temp>` without `--top` and delete the output file; otherwise rely on the shared `check_top_name` unit coverage and record the floorplan call path in `docs/results.md`.

Record in `docs/results.md` that single-top auto-accept was already present pre-change and that implementation only added coverage/documentation, unless inspection finds a concrete path that still requires `--top`.

## Files

- `crates/grus-eda/src/main.rs`
- `docs/results.md`
