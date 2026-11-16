use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "grus-eda",
    author = "Grus Systems",
    version,
    about = "Fast preflight EDA estimation toolkit for sky130hd"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// E0: Verify Liberty, LEF, DEF, and Netlist counts against OpenSTA/OpenROAD
    #[command(name = "check-e0")]
    CheckE0(CheckE0Args),

    /// E1: Estimate global-routing congestion directly from placed DEF
    Congestion(CongestionArgs),

    /// E2: Global placement + Abacus legalization, emitting placed DEF
    Place(PlaceArgs),

    /// E3: Static timing analysis (zero-wire, placement-based RC, or SPEF)
    Sta(StaArgs),
}

#[derive(Args, Debug)]
struct CheckE0Args {
    #[arg(long, help = "Path to Liberty (.lib) file")]
    lib: PathBuf,

    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: Option<PathBuf>,

    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: Option<PathBuf>,

    #[arg(long, help = "Path to DEF (.def) file")]
    def: Option<PathBuf>,

    #[arg(long, help = "Path to Yosys netlist JSON (.json) file")]
    json: Option<PathBuf>,

    #[arg(long, help = "Path to gate-level Verilog (.v) file")]
    verilog: Option<PathBuf>,

    #[arg(long, help = "Top-level module name")]
    top: Option<String>,
}

// NaN fails the range check too, since `contains` is false for NaN.
fn parse_unit_interval(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|e| format!("not a number: {e}"))?;
    if (0.0..=1.0).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{v} is outside 0.0..=1.0"))
    }
}

fn parse_target_density(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|e| format!("not a number: {e}"))?;
    if v > 0.0 && v <= 1.0 {
        Ok(v)
    } else {
        Err(format!("{v} must be in (0.0, 1.0]"))
    }
}

#[derive(Args, Debug)]
struct CongestionArgs {
    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: PathBuf,

    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: PathBuf,

    #[arg(long, help = "Path to placed DEF (.def) file")]
    def: PathBuf,

    #[arg(long, value_parser = clap::value_parser!(u32).range(1..), help = "GCell grid track supply (default: from the DEF GCELLGRID)")]
    gcell_tracks: Option<u32>,

    #[arg(
        long,
        value_parser = parse_unit_interval,
        help = "Uniform layer adjustment factor (0.0 to 1.0)"
    )]
    layer_adjust: Option<f32>,

    #[arg(long, help = "Print ASCII routing congestion heatmap")]
    heatmap: bool,
}

#[derive(Args, Debug)]
struct PlaceArgs {
    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: PathBuf,

    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: PathBuf,

    #[arg(long, help = "Path to input floorplan DEF (.def) file")]
    def: PathBuf,

    #[arg(long, help = "Path to output placed DEF (.def) file")]
    out_def: PathBuf,

    #[arg(
        long,
        default_value_t = 0.60,
        value_parser = parse_target_density,
        help = "Target placement density in (0.0, 1.0]"
    )]
    target_density: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
enum ParasiticsMode {
    None,
    Placement,
    Spef,
}

#[derive(Args, Debug)]
struct StaArgs {
    #[arg(long, help = "Path to Liberty (.lib) file")]
    lib: PathBuf,

    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: PathBuf,

    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: PathBuf,

    #[arg(long, help = "Path to placed DEF (.def) file")]
    def: PathBuf,

    #[arg(long, help = "Path to SDC (.sdc) timing constraints file")]
    sdc: PathBuf,

    #[arg(long, value_enum, help = "Parasitic interconnect model (default: zero wire unless --spef is given)")]
    parasitics: Option<ParasiticsMode>,

    #[arg(long, default_value = "met2", help = "Routing layer whose RC is used for --parasitics placement")]
    wire_layer: String,

    #[arg(long, help = "Path to extracted SPEF (.spef) file")]
    spef: Option<PathBuf>,

    #[arg(long, help = "Write the full STA report (endpoints and skip summary) as JSON to this path")]
    json: Option<PathBuf>,

    #[arg(long, help = "Exit non-zero when any cell, arc, endpoint or SDC option was skipped")]
    strict: bool,
}

fn check_top_name(expected: Option<&str>, kind: &str, actual: &str) -> anyhow::Result<()> {
    if let Some(top) = expected
        && actual != top
    {
        anyhow::bail!("{kind} design name {actual:?} does not match --top {top:?}");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    run(Cli::parse())
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::CheckE0(args) => {
            println!("Loading Liberty from {:?}", args.lib);
            let t0 = std::time::Instant::now();
            let lib_db = grus_core::readers::load_liberty(&args.lib)?;
            let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let ff_count = lib_db.cells.iter().filter(|c| c.is_sequential).count();
            let latch_count = lib_db.cells.iter().filter(|c| c.is_latch).count();
            let seq_count = ff_count + latch_count;
            println!(
                "Loaded Liberty: library={}, cells={}, pins={}, seq_cells={} (ff={}, latch={}), arcs={}, elapsed={:.2}ms",
                lib_db.library_name,
                lib_db.cells.len(),
                lib_db.pins.len(),
                seq_count,
                ff_count,
                latch_count,
                lib_db.arcs.len(),
                elapsed_ms
            );

            let has_design = args.json.is_some() || args.verilog.is_some() || args.def.is_some();
            if has_design && (args.tlef.is_none() || args.lef.is_none()) {
                anyhow::bail!("--json, --verilog, and --def require both --tlef and --lef");
            }

            if let (Some(tlef_path), Some(lef_path)) = (&args.tlef, &args.lef) {
                println!("Loading LEF from {:?} and {:?}", tlef_path, lef_path);
                let t_lef = std::time::Instant::now();
                let mut interner = grus_core::interner::StringInterner::new();
                let tech_db =
                    grus_core::readers::load_tech_db(tlef_path, lef_path, &mut interner)?;
                let elapsed_lef_ms = t_lef.elapsed().as_secs_f64() * 1000.0;
                println!(
                    "Loaded LEF: dbu/um={}, site={} ({}x{} dbu), routing_layers={}, macros={}, macro_pins={}, elapsed={:.2}ms",
                    tech_db.dbu_per_micron,
                    tech_db.site_name,
                    tech_db.site_width_dbu,
                    tech_db.site_height_dbu,
                    tech_db.routing_layers.len(),
                    tech_db.macros.len(),
                    tech_db.macro_pins.len(),
                    elapsed_lef_ms
                );

                if let Some(json_path) = &args.json {
                    println!("Loading Yosys JSON netlist from {:?}", json_path);
                    let t_json = std::time::Instant::now();
                    let db_json = grus_core::readers::parse_yosys_json(json_path, &tech_db, Some(&lib_db))?;
                    check_top_name(args.top.as_deref(), "Yosys JSON", &db_json.design_name)?;
                    let elapsed_json_ms = t_json.elapsed().as_secs_f64() * 1000.0;
                    println!(
                        "Loaded Yosys JSON: design={}, cells={}, nets={}, io_pins={}, elapsed={:.2}ms",
                        db_json.design_name,
                        db_json.num_cells(),
                        db_json.num_nets(),
                        db_json.io_pins.len(),
                        elapsed_json_ms
                    );
                }

                if let Some(v_path) = &args.verilog {
                    println!("Loading Verilog netlist from {:?}", v_path);
                    let t_v = std::time::Instant::now();
                    let db_v = grus_core::readers::parse_verilog(v_path, &tech_db, Some(&lib_db))?;
                    check_top_name(args.top.as_deref(), "Verilog", &db_v.design_name)?;
                    let elapsed_v_ms = t_v.elapsed().as_secs_f64() * 1000.0;
                    println!(
                        "Loaded Verilog: design={}, cells={}, nets={}, io_pins={}, elapsed={:.2}ms",
                        db_v.design_name,
                        db_v.num_cells(),
                        db_v.num_nets(),
                        db_v.io_pins.len(),
                        elapsed_v_ms
                    );
                }

                if let Some(def_path) = &args.def {
                    println!("Loading DEF from {:?}", def_path);
                    let t_def = std::time::Instant::now();
                    let db_def = grus_core::readers::parse_def(def_path, &tech_db, Some(&lib_db))?;
                    check_top_name(args.top.as_deref(), "DEF", &db_def.design_name)?;
                    let hpwl_dbu = db_def.compute_hpwl_dbu(&tech_db);
                    let elapsed_def_ms = t_def.elapsed().as_secs_f64() * 1000.0;
                    println!(
                        "Loaded DEF: design={}, cells={}, rows={}, tracks={}, nets={}, io_pins={}, hpwl={:.1}um ({} dbu), elapsed={:.2}ms",
                        db_def.design_name,
                        db_def.num_cells(),
                        db_def.num_rows(),
                        db_def.tracks.len(),
                        db_def.num_nets(),
                        db_def.io_pins.len(),
                        hpwl_dbu as f64 / tech_db.dbu_per_micron.max(1) as f64,
                        hpwl_dbu,
                        elapsed_def_ms
                    );
                }
            }
        }
        Commands::Congestion(args) => {
            println!("Loading technology from {:?} and {:?}", args.tlef, args.lef);
            let mut interner = grus_core::interner::StringInterner::new();
            let t_load = std::time::Instant::now();
            let tech_db = grus_core::readers::load_tech_db(&args.tlef, &args.lef, &mut interner)?;
            let design_db = grus_core::readers::parse_def(&args.def, &tech_db, None)?;
            let elapsed_load = t_load.elapsed().as_secs_f64() * 1000.0;

            let layer_adjustments = args.layer_adjust.map(|alpha| {
                (0..tech_db.routing_layers.len())
                    .map(|i| if i == 0 { 0.0f32 } else { alpha })
                    .collect::<Vec<f32>>()
            });
            let gcell_tracks_override = args.gcell_tracks.map(|t| t as i32);

            let t_route = std::time::Instant::now();
            let (grid, report) = grus_core::congestion::estimate_congestion(
                &design_db,
                &tech_db,
                layer_adjustments.as_deref(),
                gcell_tracks_override,
            );
            let elapsed_route = t_route.elapsed().as_secs_f64() * 1000.0;

            let total_cap: f32 = grid.h_cap.iter().sum::<f32>() + grid.v_cap.iter().sum::<f32>();
            let total_demand: f32 = grid.h_demand.iter().sum::<f32>() + grid.v_demand.iter().sum::<f32>();

            println!(
                "Congestion Estimation: design={}, grid={}x{}, tile_w={} dbu, tile_h={} dbu",
                design_db.design_name, grid.nx, grid.ny, grid.gcell_w_dbu, grid.gcell_h_dbu
            );
            println!(
                "Routing Resources: total_capacity={:.0}, total_demand={:.0}, total_overflow={:.1}",
                total_cap, total_demand, report.total_overflow
            );
            println!(
                "Congestion Metrics: max_edge_overflow={:.1}, top_05pct_overflow={:.2}, overflow_gcells={}/{}",
                report.max_edge_overflow, report.top_05pct_overflow, report.overflow_gcells, grid.nx * grid.ny
            );
            println!(
                "Runtime: load={:.2}ms, estimation={:.2}ms",
                elapsed_load, elapsed_route
            );

            if args.heatmap {
                print_heatmap(grid.nx, grid.ny, &report.gcell_usage_ratio);
            }
        }
        Commands::Place(args) => {
            println!("Loading technology from {:?} and {:?}", args.tlef, args.lef);
            let mut interner = grus_core::interner::StringInterner::new();
            let t_load = std::time::Instant::now();
            let tech_db = grus_core::readers::load_tech_db(&args.tlef, &args.lef, &mut interner)?;
            let mut design_db = grus_core::readers::parse_def(&args.def, &tech_db, None)?;
            let elapsed_load = t_load.elapsed().as_secs_f64() * 1000.0;
            let movable_count = design_db
                .cell_status
                .iter()
                .zip(design_db.cell_macros.iter())
                .filter(|&(&status, &macro_id)| {
                    matches!(
                        status,
                        grus_core::design_db::PlacementStatus::Unplaced
                            | grus_core::design_db::PlacementStatus::Placed
                    ) && matches!(
                        tech_db.macros[macro_id.0 as usize].class,
                        grus_core::tech_db::MacroClass::Core
                            | grus_core::tech_db::MacroClass::CoreTie
                    )
                })
                .count();
            println!(
                "Loaded design {}: {} cells ({} movable), {} nets, {} rows, elapsed={:.2}ms",
                design_db.design_name,
                design_db.num_cells(),
                movable_count,
                design_db.num_nets(),
                design_db.num_rows(),
                elapsed_load
            );

            // Global Placement (Nesterov Accelerated Gradient)
            println!(
                "Running global placement (target density: {:.2})...",
                args.target_density
            );
            let config = grus_core::placement::NesterovConfig {
                target_density: args.target_density,
                ..Default::default()
            };
            let t_gp = std::time::Instant::now();
            let gp_res = grus_core::placement::run_nesterov_placement(&design_db, &tech_db, &config);
            let elapsed_gp = t_gp.elapsed().as_secs_f64() * 1000.0;
            if gp_res.converged {
                println!(
                    "Global placement: converged in {} iters, final_overflow={:.4}, final_hpwl={:.1}um, elapsed={:.2}ms ({:.1}us/iter)",
                    gp_res.iterations,
                    gp_res.final_overflow,
                    gp_res.final_hpwl_um,
                    elapsed_gp,
                    (elapsed_gp * 1000.0) / gp_res.iterations.max(1) as f64
                );
            } else {
                println!(
                    "Global placement: did not converge (iteration limit reached) in {} iters, final_overflow={:.4}, final_hpwl={:.1}um, elapsed={:.2}ms ({:.1}us/iter)",
                    gp_res.iterations,
                    gp_res.final_overflow,
                    gp_res.final_hpwl_um,
                    elapsed_gp,
                    (elapsed_gp * 1000.0) / gp_res.iterations.max(1) as f64
                );
                eprintln!(
                    "warning: global placement did not converge (final_overflow={:.4}); the placed DEF is an estimate",
                    gp_res.final_overflow
                );
            }

            // Detailed Placement (Abacus Legalization + Local Refinement)
            println!("Running legalization and detailed placement refinement...");
            let t_leg = std::time::Instant::now();
            let leg_res = grus_core::placement::legalize_placement(
                &mut design_db,
                &tech_db,
                &gp_res.cell_x_um,
                &gp_res.cell_y_um,
            )?;
            let elapsed_leg = t_leg.elapsed().as_secs_f64() * 1000.0;
            println!(
                "Legalization: pre_legal_hpwl={} dbu, post_abacus_hpwl={} dbu, final_hpwl={} dbu ({:.1}um), avg_disp={:.2}um, max_disp={:.2}um, elapsed={:.2}ms",
                leg_res.pre_legal_hpwl_dbu,
                leg_res.post_abacus_hpwl_dbu,
                leg_res.final_hpwl_dbu,
                leg_res.final_hpwl_dbu as f64 / tech_db.dbu_per_micron as f64,
                leg_res.avg_displacement_um,
                leg_res.max_displacement_um,
                elapsed_leg
            );

            // Write placed DEF
            println!("Writing placed DEF to {:?}", args.out_def);
            let t_write = std::time::Instant::now();
            grus_core::readers::write_def(&design_db, &tech_db, &args.def, &args.out_def)?;
            let elapsed_write = t_write.elapsed().as_secs_f64() * 1000.0;
            println!(
                "Wrote placed DEF successfully in {:.2}ms (total placement runtime: {:.2}ms)",
                elapsed_write,
                elapsed_gp + elapsed_leg
            );
        }
        Commands::Sta(args) => {
            let mode = match (args.parasitics, &args.spef) {
                (None | Some(ParasiticsMode::None), None) => "zero wire",
                (Some(ParasiticsMode::Placement), None) => "placement RC",
                (None | Some(ParasiticsMode::Spef), Some(_)) => "SPEF",
                (parasitics, spef) => anyhow::bail!("unsupported --parasitics {:?} with --spef {:?}", parasitics, spef),
            };
            let lib_db = grus_core::readers::load_liberty(&args.lib)?;
            let mut interner = grus_core::interner::StringInterner::new();
            let tech_db = grus_core::readers::load_tech_db(&args.tlef, &args.lef, &mut interner)?;
            let design = grus_core::readers::parse_def(&args.def, &tech_db, Some(&lib_db))?;
            let sdc = grus_core::sta::load_sdc(&args.sdc)?;
            let t0 = std::time::Instant::now();
            let parasitics = match (mode, &args.spef) {
                ("placement RC", _) => {
                    let rc = grus_core::sta::WireRc::from_layer(&tech_db, &args.wire_layer)?;
                    Some(grus_core::sta::estimate_placement_parasitics(&design, &tech_db, rc))
                }
                (_, Some(spef_path)) => {
                    let spef = grus_core::readers::load_spef(spef_path)?;
                    Some(grus_core::sta::spef_parasitics(&design, &spef)?)
                }
                _ => None,
            };
            let report = grus_core::sta::run_sta(&design, &lib_db, &sdc, parasitics.as_ref())?;
            println!(
                "STA ({}): endpoints={}, setup wns={:.6} tns={:.6} viol={}, hold wns={:.6} tns={:.6} viol={}, elapsed={:.2}ms",
                mode,
                report.endpoints.len(),
                report.wns_max_ns,
                report.tns_max_ns,
                report.setup_violations,
                report.wns_min_ns,
                report.tns_min_ns,
                report.hold_violations,
                t0.elapsed().as_secs_f64() * 1000.0,
            );
            print_skip_summary(&report);
            if let Some(path) = &args.json {
                let json = serde_json::to_string_pretty(&report)?;
                std::fs::write(path, json)?;
                println!("Wrote STA report JSON to {:?}", path);
            }
            if args.strict && !report.skipped.is_clean() {
                anyhow::bail!("strict: the run skipped inputs; see the summary above");
            }
        }
    }
    Ok(())
}

/// Per-kind counts of everything the STA run left out, as a share of the design's arcs and
/// endpoints plus the ignored SDC options. Printed at the end of every `sta` run.
fn print_skip_summary(report: &grus_core::sta::StaReport) {
    let s = &report.skipped;
    if s.is_clean() {
        println!("skipped: none");
        return;
    }
    let share = |n: usize, total: usize| {
        if total == 0 { "n/a".to_string() } else { format!("{:.2}%", 100.0 * n as f64 / total as f64) }
    };
    println!(
        "skipped: {}/{} arcs ({}), {}/{} endpoints ({})",
        s.arcs_skipped(),
        s.total_arcs,
        share(s.arcs_skipped(), s.total_arcs),
        s.endpoints_skipped(),
        s.total_endpoints,
        share(s.endpoints_skipped(), s.total_endpoints),
    );
    if s.sdc_commands > 0 {
        let ignored: usize = s.sdc_ignored_options.values().sum();
        if ignored > 0 {
            println!(
                "  sdc: {} ignored option(s) of {} commands ({:.2}%)",
                ignored,
                s.sdc_commands,
                100.0 * ignored as f64 / s.sdc_commands as f64
            );
        }
    }
    for category in s.nonzero() {
        println!("  {category}");
    }
}

fn print_heatmap(grid_w: usize, grid_h: usize, ratios: &[f32]) {
    println!("Routing Congestion Heatmap ({}x{}):", grid_w, grid_h);
    println!("Legend: '.' < 50%, '-' < 80%, '+' < 100%, '*' 100-120%, '#' > 120%");
    for y in (0..grid_h).rev() {
        print!("{:2} | ", y);
        for x in 0..grid_w {
            let r = ratios[y * grid_w + x];
            let ch = if r < 0.50 {
                '.'
            } else if r < 0.80 {
                '-'
            } else if r <= 1.00 {
                '+'
            } else if r <= 1.20 {
                '*'
            } else {
                '#'
            };
            print!("{}", ch);
        }
        println!();
    }
    print!("     ");
    for x in 0..grid_w {
        print!("{}", x % 10);
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sky130_hd() -> PathBuf {
        if let Some(root) = std::env::var_os("PDK_ROOT") {
            return PathBuf::from(root).join("sky130A/libs.ref/sky130_fd_sc_hd");
        }
        let home = std::env::var_os("HOME").expect("HOME or PDK_ROOT must be set");
        PathBuf::from(home).join(".ciel/sky130A/libs.ref/sky130_fd_sc_hd")
    }

    fn require(p: PathBuf) -> PathBuf {
        assert!(p.exists(), "required test input missing: {}", p.display());
        p
    }

    #[test]
    fn cli_parsers_reject_out_of_range_and_nan() {
        assert!(Cli::try_parse_from(["grus-eda", "congestion", "--tlef", "a", "--lef", "b", "--def", "c", "--layer-adjust", "1.5"]).is_err());
        assert!(Cli::try_parse_from(["grus-eda", "congestion", "--tlef", "a", "--lef", "b", "--def", "c", "--layer-adjust", "NaN"]).is_err());
        assert!(Cli::try_parse_from(["grus-eda", "congestion", "--tlef", "a", "--lef", "b", "--def", "c", "--gcell-tracks", "0"]).is_err());
        assert!(Cli::try_parse_from(["grus-eda", "place", "--tlef", "a", "--lef", "b", "--def", "c", "--out-def", "d", "--target-density", "0"]).is_err());
        assert!(Cli::try_parse_from(["grus-eda", "place", "--tlef", "a", "--lef", "b", "--def", "c", "--out-def", "d", "--target-density", "1.1"]).is_err());
        assert!(Cli::try_parse_from(["grus-eda", "place", "--tlef", "a", "--lef", "b", "--def", "c", "--out-def", "d", "--target-density", "NaN"]).is_err());
        assert!(Cli::try_parse_from(["grus-eda", "place", "--tlef", "a", "--lef", "b", "--def", "c", "--out-def", "d", "--target-density", "0.60"]).is_ok());
    }

    #[test]
    fn cli_end_to_end_gcd_flow_and_validation() {
        let hd = sky130_hd();
        let lib = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let gcd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/designs/gcd");
        let out_def = std::env::temp_dir().join("grus_cli_test_gcd_placed.def");

        // 1. --top mismatches and missing --tlef/--lef must fail loudly
        let err_missing_lef = run(Cli::try_parse_from([
            "grus-eda", "check-e0", "--lib", lib.to_str().unwrap(),
            "--def", gcd.join("gcd.placed.def").to_str().unwrap(),
        ]).unwrap());
        assert!(err_missing_lef.is_err(), "check-e0 with --def but no --tlef/--lef must fail");

        let err_top = run(Cli::try_parse_from([
            "grus-eda", "check-e0",
            "--lib", lib.to_str().unwrap(),
            "--tlef", tlef.to_str().unwrap(),
            "--lef", lef.to_str().unwrap(),
            "--json", gcd.join("gcd.synth.json").to_str().unwrap(),
            "--top", "wrong_module",
        ]).unwrap());
        assert!(err_top.is_err(), "mismatched --top must fail");

        // 2. E0: check-e0 across Liberty, LEF, JSON, Verilog, DEF with matching --top
        run(Cli::try_parse_from([
            "grus-eda", "check-e0",
            "--lib", lib.to_str().unwrap(),
            "--tlef", tlef.to_str().unwrap(),
            "--lef", lef.to_str().unwrap(),
            "--json", gcd.join("gcd.synth.json").to_str().unwrap(),
            "--verilog", gcd.join("gcd.synth.v").to_str().unwrap(),
            "--def", gcd.join("gcd.placed.def").to_str().unwrap(),
            "--top", "gcd",
        ]).unwrap()).expect("check-e0 must succeed");

        // 3. E2: place -> out_def
        run(Cli::try_parse_from([
            "grus-eda", "place",
            "--tlef", tlef.to_str().unwrap(),
            "--lef", lef.to_str().unwrap(),
            "--def", gcd.join("gcd.floorplan.def").to_str().unwrap(),
            "--out-def", out_def.to_str().unwrap(),
        ]).unwrap()).expect("place must succeed");

        // 4. E1: congestion on newly placed DEF with --heatmap and --layer-adjust
        run(Cli::try_parse_from([
            "grus-eda", "congestion",
            "--tlef", tlef.to_str().unwrap(),
            "--lef", lef.to_str().unwrap(),
            "--def", out_def.to_str().unwrap(),
            "--layer-adjust", "0.2",
            "--heatmap",
        ]).unwrap()).expect("congestion must succeed");

        // 5. E3: zero-wire, placement RC on placed out_def, and routed SPEF
        let sdc_p = gcd.join("gcd.sdc");
        let spef_p = gcd.join("gcd.routed.spef");
        for extra in [
            vec![],
            vec!["--parasitics", "placement"],
            vec!["--parasitics", "spef", "--spef", spef_p.to_str().unwrap()],
        ] {
            let mut argv = vec![
                "grus-eda", "sta",
                "--lib", lib.to_str().unwrap(),
                "--tlef", tlef.to_str().unwrap(),
                "--lef", lef.to_str().unwrap(),
                "--def", out_def.to_str().unwrap(),
                "--sdc", sdc_p.to_str().unwrap(),
            ];
            argv.extend(extra);
            run(Cli::try_parse_from(argv).unwrap()).expect("sta must succeed");
        }

        // Reject contradictory --parasitics placement --spef
        let err_sta = run(Cli::try_parse_from([
            "grus-eda", "sta",
            "--lib", lib.to_str().unwrap(),
            "--tlef", tlef.to_str().unwrap(),
            "--lef", lef.to_str().unwrap(),
            "--def", out_def.to_str().unwrap(),
            "--sdc", sdc_p.to_str().unwrap(),
            "--parasitics", "placement",
            "--spef", spef_p.to_str().unwrap(),
        ]).unwrap());
        assert!(err_sta.is_err(), "contradictory --parasitics placement --spef must fail");

        let _ = std::fs::remove_file(&out_def);
    }

    #[test]
    fn cli_sta_json_and_strict() {
        let hd = sky130_hd();
        let lib = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let gcd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/designs/gcd");
        let def = gcd.join("gcd.placed.def");
        let sdc = gcd.join("gcd.sdc");
        let sta_argv = |extra: &[&str]| {
            let mut argv = vec![
                "grus-eda", "sta",
                "--lib", lib.to_str().unwrap(),
                "--tlef", tlef.to_str().unwrap(),
                "--lef", lef.to_str().unwrap(),
                "--def", def.to_str().unwrap(),
                "--sdc", sdc.to_str().unwrap(),
            ];
            argv.extend_from_slice(extra);
            Cli::try_parse_from(argv).unwrap()
        };

        // gcd.placed.def has 93 unmapped welltap cells, so --strict must fail.
        let json_path = std::env::temp_dir().join("grus_cli_test_sta.json");
        run(sta_argv(&["--json", json_path.to_str().unwrap()])).expect("sta must succeed");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json_path).expect("report JSON")).expect("JSON parses");
        assert_eq!(report["skipped"]["unmapped_cells"].as_u64().unwrap(), 93);
        assert!(report["skipped"]["total_arcs"].as_u64().unwrap() > 0);

        let err = run(sta_argv(&["--strict"]));
        assert!(err.is_err(), "--strict must fail when cells were skipped");

        let _ = std::fs::remove_file(&json_path);
    }
}
