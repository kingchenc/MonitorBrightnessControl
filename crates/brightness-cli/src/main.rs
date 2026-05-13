//! `mbc` — the Monitor Brightness Control CLI.
//!
//! Wraps `brightness-core` so the same backend that powers the Tauri app is
//! reachable from scripts, integration tests, and ad-hoc debugging.

use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, anyhow};
use brightness_core::{
    monitor::{Monitor, MonitorManager},
    platform::default_manager,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "mbc",
    version,
    about = "Cross-platform monitor brightness and DDC/CI control."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human, global = true)]
    format: OutputFormat,

    /// Increase log verbosity. Repeat for more (-v, -vv).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List all monitors.
    List,
    /// Print brightness for one or all monitors.
    Get(SelectArgs),
    /// Set brightness percentage (0..=100) on one or all monitors.
    Set {
        #[command(flatten)]
        sel: SelectArgs,
        /// Brightness 0..=100.
        value: f32,
    },
    /// Get a raw VCP feature.
    GetVcp {
        #[command(flatten)]
        sel: SelectArgs,
        /// VCP code, hex (e.g. 0x10) or decimal.
        code: HexU8,
    },
    /// Set a raw VCP feature.
    SetVcp {
        #[command(flatten)]
        sel: SelectArgs,
        /// VCP code, hex (e.g. 0x10) or decimal.
        code: HexU8,
        /// 16-bit value to write.
        value: u16,
    },
    /// Print MCCS capability string for a monitor.
    Capabilities(SelectArgs),
    /// Adjust by a delta (positive or negative percentage points).
    Step {
        #[command(flatten)]
        sel: SelectArgs,
        /// Delta in percentage points (e.g. 10 or -5).
        delta: f32,
    },
    /// Run a latency benchmark across all monitors.
    Benchmark {
        /// Number of iterations per monitor.
        #[arg(long, default_value_t = 5)]
        iterations: u32,
    },
}

#[derive(Args, Debug)]
struct SelectArgs {
    /// Target monitor by id (use `mbc list` to see ids). Omit to act on all.
    #[arg(long)]
    id: Option<String>,
}

#[derive(Debug, Clone)]
struct HexU8(u8);

impl std::str::FromStr for HexU8 {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_trim = s.trim();
        let parsed = if let Some(h) = s_trim
            .strip_prefix("0x")
            .or_else(|| s_trim.strip_prefix("0X"))
        {
            u8::from_str_radix(h, 16)
        } else if s_trim.chars().all(|c| c.is_ascii_digit()) {
            s_trim.parse::<u8>()
        } else {
            u8::from_str_radix(s_trim, 16)
        };
        parsed.map(HexU8).map_err(|e| format!("bad VCP code: {e}"))
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let _ = env_logger::Builder::new()
        .parse_filters(level)
        .format_timestamp(None)
        .try_init();
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let mgr = default_manager().context("initializing platform backend")?;
    let monitors = mgr.list().context("listing monitors")?;

    match cli.command {
        Command::List => print_list(&monitors, cli.format),
        Command::Get(sel) => cmd_get(&monitors, sel, cli.format),
        Command::Set { sel, value } => cmd_set(&monitors, sel, value, cli.format),
        Command::GetVcp { sel, code } => cmd_get_vcp(&monitors, sel, code.0, cli.format),
        Command::SetVcp { sel, code, value } => {
            cmd_set_vcp(&monitors, sel, code.0, value, cli.format)
        }
        Command::Capabilities(sel) => cmd_caps(&monitors, sel, cli.format),
        Command::Step { sel, delta } => cmd_step(&monitors, sel, delta, cli.format),
        Command::Benchmark { iterations } => cmd_benchmark(mgr.as_ref(), iterations, cli.format),
    }
}

fn select<'a>(monitors: &'a [Monitor], sel: &SelectArgs) -> anyhow::Result<Vec<&'a Monitor>> {
    if let Some(id) = &sel.id {
        let m = monitors
            .iter()
            .find(|m| m.info().id.as_str() == id)
            .ok_or_else(|| anyhow!("no monitor with id {id}"))?;
        Ok(vec![m])
    } else {
        Ok(monitors.iter().collect())
    }
}

#[derive(Serialize)]
struct ListItemJson<'a> {
    id: &'a str,
    name: &'a str,
    kind: String,
}

fn print_list(monitors: &[Monitor], format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Human => {
            for m in monitors {
                let info = m.info();
                println!("{}\t{}\t[{}]", info.id, info.name, info.kind);
            }
        }
        OutputFormat::Json => {
            let v: Vec<_> = monitors
                .iter()
                .map(|m| ListItemJson {
                    id: m.info().id.as_str(),
                    name: &m.info().name,
                    kind: m.info().kind.to_string(),
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct BrightnessRow<'a> {
    id: &'a str,
    name: &'a str,
    percent: Option<f32>,
    error: Option<String>,
}

fn cmd_get(monitors: &[Monitor], sel: SelectArgs, format: OutputFormat) -> anyhow::Result<()> {
    let targets = select(monitors, &sel)?;
    let mut rows: Vec<BrightnessRow> = Vec::new();
    for m in targets {
        match m.get_brightness_percent() {
            Ok(p) => rows.push(BrightnessRow {
                id: m.info().id.as_str(),
                name: &m.info().name,
                percent: Some(p),
                error: None,
            }),
            Err(e) => rows.push(BrightnessRow {
                id: m.info().id.as_str(),
                name: &m.info().name,
                percent: None,
                error: Some(e.to_string()),
            }),
        }
    }
    print_brightness_rows(&rows, format)
}

fn cmd_set(
    monitors: &[Monitor],
    sel: SelectArgs,
    value: f32,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let targets = select(monitors, &sel)?;
    let mut rows: Vec<BrightnessRow> = Vec::new();
    for m in targets {
        let res = m.set_brightness_percent(value);
        let percent = m.get_brightness_percent().ok();
        rows.push(BrightnessRow {
            id: m.info().id.as_str(),
            name: &m.info().name,
            percent,
            error: res.err().map(|e| e.to_string()),
        });
    }
    print_brightness_rows(&rows, format)
}

fn cmd_step(
    monitors: &[Monitor],
    sel: SelectArgs,
    delta: f32,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let targets = select(monitors, &sel)?;
    let mut rows: Vec<BrightnessRow> = Vec::new();
    for m in targets {
        let cur = m.get_brightness_percent().unwrap_or(50.0);
        let next = (cur + delta).clamp(0.0, 100.0);
        let res = m.set_brightness_percent(next);
        rows.push(BrightnessRow {
            id: m.info().id.as_str(),
            name: &m.info().name,
            percent: Some(next),
            error: res.err().map(|e| e.to_string()),
        });
    }
    print_brightness_rows(&rows, format)
}

fn print_brightness_rows(rows: &[BrightnessRow], format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Human => {
            for r in rows {
                if let Some(e) = &r.error {
                    println!("{}\t{}\tERROR: {}", r.id, r.name, e);
                } else if let Some(p) = r.percent {
                    println!("{}\t{}\t{:.1}%", r.id, r.name, p);
                } else {
                    println!("{}\t{}\t-", r.id, r.name);
                }
            }
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(rows)?);
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct VcpRow<'a> {
    id: &'a str,
    name: &'a str,
    code: u8,
    current: Option<u16>,
    maximum: Option<u16>,
    error: Option<String>,
}

fn cmd_get_vcp(
    monitors: &[Monitor],
    sel: SelectArgs,
    code: u8,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let targets = select(monitors, &sel)?;
    let mut rows = Vec::new();
    for m in targets {
        match m.get_vcp(code) {
            Ok(v) => rows.push(VcpRow {
                id: m.info().id.as_str(),
                name: &m.info().name,
                code,
                current: Some(v.current),
                maximum: Some(v.maximum),
                error: None,
            }),
            Err(e) => rows.push(VcpRow {
                id: m.info().id.as_str(),
                name: &m.info().name,
                code,
                current: None,
                maximum: None,
                error: Some(e.to_string()),
            }),
        }
    }
    match format {
        OutputFormat::Human => {
            for r in rows {
                if let Some(e) = r.error {
                    println!("{}\t{}\tVCP 0x{:02X}\tERROR: {}", r.id, r.name, r.code, e);
                } else {
                    println!(
                        "{}\t{}\tVCP 0x{:02X}\t{}/{}",
                        r.id,
                        r.name,
                        r.code,
                        r.current.unwrap_or(0),
                        r.maximum.unwrap_or(0)
                    );
                }
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
    }
    Ok(())
}

fn cmd_set_vcp(
    monitors: &[Monitor],
    sel: SelectArgs,
    code: u8,
    value: u16,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let targets = select(monitors, &sel)?;
    let mut rows = Vec::new();
    for m in targets {
        let res = m.set_vcp(code, value);
        let after = m.get_vcp(code).ok();
        rows.push(VcpRow {
            id: m.info().id.as_str(),
            name: &m.info().name,
            code,
            current: after.as_ref().map(|v| v.current),
            maximum: after.as_ref().map(|v| v.maximum),
            error: res.err().map(|e| e.to_string()),
        });
    }
    match format {
        OutputFormat::Human => {
            for r in rows {
                if let Some(e) = r.error {
                    println!("{}\t{}\tSET 0x{:02X}\tERROR: {}", r.id, r.name, r.code, e);
                } else {
                    println!(
                        "{}\t{}\tSET 0x{:02X}={value}\tnow {}/{}",
                        r.id,
                        r.name,
                        r.code,
                        r.current.unwrap_or(0),
                        r.maximum.unwrap_or(0)
                    );
                }
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
    }
    Ok(())
}

#[derive(Serialize)]
struct CapsRow<'a> {
    id: &'a str,
    name: &'a str,
    capabilities: Option<String>,
    parsed: Option<brightness_core::caps::Capabilities>,
    error: Option<String>,
}

fn cmd_caps(monitors: &[Monitor], sel: SelectArgs, format: OutputFormat) -> anyhow::Result<()> {
    let targets = select(monitors, &sel)?;
    let mut rows = Vec::new();
    for m in targets {
        match m.capabilities() {
            Ok(c) => rows.push(CapsRow {
                id: m.info().id.as_str(),
                name: &m.info().name,
                capabilities: Some(c.raw.clone()),
                parsed: Some(c),
                error: None,
            }),
            Err(e) => rows.push(CapsRow {
                id: m.info().id.as_str(),
                name: &m.info().name,
                capabilities: None,
                parsed: None,
                error: Some(e.to_string()),
            }),
        }
    }
    match format {
        OutputFormat::Human => {
            for r in rows {
                if let Some(e) = r.error {
                    println!("{}\t{}\tERROR: {}", r.id, r.name, e);
                } else {
                    println!("{}\t{}", r.id, r.name);
                    if let Some(c) = r.capabilities {
                        println!("{c}");
                    }
                    if let Some(p) = r.parsed {
                        println!(
                            "  vcp codes: {}",
                            p.vcp
                                .keys()
                                .map(|c| format!("0x{c:02X}"))
                                .collect::<Vec<_>>()
                                .join(" ")
                        );
                    }
                }
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
    }
    Ok(())
}

#[derive(Serialize)]
struct BenchRow<'a> {
    id: &'a str,
    name: &'a str,
    iterations: u32,
    avg_set_us: u128,
    avg_get_us: u128,
    error: Option<String>,
}

fn cmd_benchmark(
    mgr: &dyn MonitorManager,
    iterations: u32,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let monitors = mgr.list()?;
    let mut rows = Vec::<BenchRow>::new();
    for m in &monitors {
        let info = m.info();
        let baseline = m.get_brightness_percent();
        if let Err(e) = baseline {
            rows.push(BenchRow {
                id: info.id.as_str(),
                name: &info.name,
                iterations,
                avg_set_us: 0,
                avg_get_us: 0,
                error: Some(format!("baseline read failed: {e}")),
            });
            continue;
        }
        let baseline = baseline.unwrap();
        let mut total_set = 0u128;
        let mut total_get = 0u128;
        let mut err: Option<String> = None;
        for i in 0..iterations {
            let target = if i % 2 == 0 { 50.0 } else { 60.0 };
            let t0 = Instant::now();
            if let Err(e) = m.set_brightness_percent(target) {
                err = Some(format!("set failed: {e}"));
                break;
            }
            total_set += t0.elapsed().as_micros();
            let t0 = Instant::now();
            if let Err(e) = m.get_brightness_percent() {
                err = Some(format!("get failed: {e}"));
                break;
            }
            total_get += t0.elapsed().as_micros();
        }
        // Restore.
        let _ = m.set_brightness_percent(baseline);
        rows.push(BenchRow {
            id: info.id.as_str(),
            name: &info.name,
            iterations,
            avg_set_us: total_set / iterations.max(1) as u128,
            avg_get_us: total_get / iterations.max(1) as u128,
            error: err,
        });
    }
    match format {
        OutputFormat::Human => {
            println!("monitor\tavg_set\tavg_get");
            for r in rows {
                if let Some(e) = r.error {
                    println!("{}\t{}\tERROR: {}", r.id, r.name, e);
                } else {
                    println!(
                        "{}\t{}\t{:.2}ms\t{:.2}ms",
                        r.id,
                        r.name,
                        r.avg_set_us as f64 / 1000.0,
                        r.avg_get_us as f64 / 1000.0
                    );
                }
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
    }
    Ok(())
}
