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

    /// E2: Global placement + greedy legalization, emitting placed DEF
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

#[derive(Args, Debug)]
struct CongestionArgs {
    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: PathBuf,

    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: PathBuf,

    #[arg(long, help = "Path to placed DEF (.def) file")]
    def: PathBuf,

    #[arg(long, default_value_t = 15, help = "GCell grid track supply")]
    gcell_tracks: u32,

    #[arg(long, help = "Uniform layer adjustment factor (0.0 to 1.0)")]
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

    #[arg(long, default_value_t = 0.60, help = "Target placement density (0.0 to 1.0)")]
    target_density: f32,
}

#[derive(Args, Debug)]
struct StaArgs {
    #[arg(long, help = "Path to Liberty (.lib) file")]
    lib: PathBuf,

    #[arg(long, help = "Path to Yosys netlist JSON (.json) file")]
    json: Option<PathBuf>,

    #[arg(long, help = "Path to gate-level Verilog (.v) file")]
    verilog: Option<PathBuf>,

    #[arg(long, help = "Path to placed DEF (.def) file")]
    def: Option<PathBuf>,

    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: Option<PathBuf>,

    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: Option<PathBuf>,

    #[arg(long, help = "Parasitic interconnect model (none, placement)")]
    parasitics: Option<String>,

    #[arg(long, help = "Path to extracted SPEF (.spef) file")]
    spef: Option<PathBuf>,

    #[arg(long, help = "Path to SDC (.sdc) timing constraints file")]
    sdc: Option<PathBuf>,

    #[arg(long, help = "Clock port name (if SDC omitted)")]
    clock_pin: Option<String>,

    #[arg(long, help = "Clock period in nanoseconds (if SDC omitted)")]
    period_ns: Option<f32>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
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
                        hpwl_dbu as f64 / 1000.0,
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
                vec![0.0f32, alpha, alpha, alpha, alpha, alpha]
            });
            let gcell_tracks_override = if args.gcell_tracks != 15 {
                Some(args.gcell_tracks as i32)
            } else {
                None
            };

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
            println!(
                "grus-eda place: def={:?}, out_def={:?}, target_density={}",
                args.def, args.out_def, args.target_density
            );
        }
        Commands::Sta(args) => {
            println!("grus-eda sta: lib={:?}", args.lib);
        }
    }
    Ok(())
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
