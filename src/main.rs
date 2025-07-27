use std::process;
use std::fs::{self, File};
use std::path::PathBuf;
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex};

use clap::Parser;
use chrono::Local;
use indicatif::{ProgressBar, ProgressStyle};
use log::{info, error};
use rand::random_range;
use rayon::prelude::*;

#[derive(Parser, Debug)]
#[command(name = "Coin Flip Simulator")]
#[command(version = "3.4", author = "Ursus")]
#[command(about = "Simulates (un)fair coin flips in parallel and streams results directly to CSV")]
struct Args {
    #[arg(short, long)]
    trials: u64,

    #[arg(short, long)]
    output: Option<String>,

    #[arg(long)]
    no_csv: bool,

    /// Number of threads to use (default: 1 = single-threaded)
    #[arg(short = 'j', long, default_value_t = 1)]
    jobs: usize,

    /// Chance of heads in percentage (e.g. 70.0 = 70% heads)
    #[arg(long, default_value_t = 50.0)]
    bias: f64,

    /// Show colored terminal summary
    #[arg(long, default_value_t = false)]
    color: bool,

    /// Show ASCII bar chart in summary
    #[arg(long, default_value_t = false)]
    chart: bool,

    /// Suppress terminal summary output
    #[arg(long, default_value_t = false)]
    quiet: bool,
}

struct CoinFlipStreamer {
    trials: u64,
    head_count: Arc<Mutex<u64>>,
    tail_count: Arc<Mutex<u64>>,
}

impl CoinFlipStreamer {
    fn new(trials: u64) -> Self {
        Self {
            trials,
            head_count: Arc::new(Mutex::new(0)),
            tail_count: Arc::new(Mutex::new(0)),
        }
    }

    fn run_parallel(&self, csv_mutex: Option<Arc<Mutex<BufWriter<File>>>>, jobs: usize, bias: f64) {
        info!("Starting simulation with {} trials, bias {}%, using {} thread(s)", self.trials, bias, jobs);

        let pb = ProgressBar::new(self.trials);
        pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent}% {msg}")
            .unwrap()
            .progress_chars("=>-"));

        if jobs > 1 {
            rayon::ThreadPoolBuilder::new()
                .num_threads(jobs)
                .build_global()
                .unwrap_or_else(|e| {
                    error!("Failed to set thread pool: {}", e);
                    process::exit(1);
                });

            (1..=self.trials).into_par_iter().for_each(|trial| {
                self.process_trial(trial, bias, &csv_mutex, &pb);
            });
        } else {
            for trial in 1..=self.trials {
                self.process_trial(trial, bias, &csv_mutex, &pb);
            }
        }

        pb.finish_with_message("Done");
        info!("Simulation complete");
    }

    fn process_trial(
        &self,
        trial: u64,
        bias: f64,
        csv_mutex: &Option<Arc<Mutex<BufWriter<File>>>>,
        pb: &ProgressBar,
    ) {
        let value = random_range(0..100);
        let (outcome_str, is_head) = if value < bias.round() as u32 {
            ("H".to_string(), true)
        } else {
            ("T".to_string(), false)
        };

        if let Some(file_mutex) = csv_mutex {
            if let Ok(mut file) = file_mutex.lock() {
                let _ = writeln!(file, "{},{},{}", trial, outcome_str, value);
            }
        }

        if is_head {
            let mut heads = self.head_count.lock().unwrap();
            *heads += 1;
        } else {
            let mut tails = self.tail_count.lock().unwrap();
            *tails += 1;
        }

        pb.inc(1);
    }

    fn export_summary(&self, filename: &str, total: u64, bias: f64) -> Result<(), Box<dyn std::error::Error>> {
        let heads = *self.head_count.lock().unwrap();
        let tails = *self.tail_count.lock().unwrap();
        let head_pct = 100.0 * heads as f64 / total as f64;
        let tail_pct = 100.0 * tails as f64 / total as f64;

        info!("Writing summary to {}", filename);
        let mut file = File::create(filename)?;
        writeln!(file, "Summary Report")?;
        writeln!(file, "==============")?;
        writeln!(file, "Total Trials: {}", total)?;
        writeln!(file, "Bias: {:.2}%", bias)?;
        writeln!(file, "Heads: {} ({:.2}%)", heads, head_pct)?;
        writeln!(file, "Tails: {} ({:.2}%)", tails, tail_pct)?;
        info!("Summary report written.");
        Ok(())
    }

    fn print_summary_to_terminal(&self, total: u64, bias: f64, color: bool, chart: bool) {
        let heads = *self.head_count.lock().unwrap();
        let tails = *self.tail_count.lock().unwrap();
        let head_pct = 100.0 * heads as f64 / total as f64;
        let tail_pct = 100.0 * tails as f64 / total as f64;

        let (h_label, t_label) = if color {
            (
                format!("\x1b[32mHeads: {} ({:.2}%)\x1b[0m", heads, head_pct),
                format!("\x1b[31mTails: {} ({:.2}%)\x1b[0m", tails, tail_pct),
            )
        } else {
            (
                format!("Heads: {} ({:.2}%)", heads, head_pct),
                format!("Tails: {} ({:.2}%)", tails, tail_pct),
            )
        };

        println!("\nSummary Report");
        println!("==============");
        println!("Total Trials: {}", total);
        println!("Bias: {:.2}%", bias);
        println!("{}", h_label);
        println!("{}", t_label);

        if chart {
            let bar_len = 40;
            let head_bar = (head_pct / 100.0 * bar_len as f64).round() as usize;
            let tail_bar = bar_len - head_bar;

            println!(
                "\n[{}{}]",
                "🟩".repeat(head_bar),
                "🟥".repeat(tail_bar)
            );
        }
    }
}

fn sanitize_filename(name: &str) -> String {
    name.replace("../", "").replace("/", "_")
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    if args.trials == 0 {
        eprintln!("Please provide a positive number of trials.");
        process::exit(1);
    }

    if args.bias < 0.0 || args.bias > 100.0 {
        eprintln!("Bias must be between 0.0 and 100.0");
        process::exit(1);
    }

    let output_dir = PathBuf::from("output");
    if !output_dir.exists() {
        if let Err(e) = fs::create_dir(&output_dir) {
            eprintln!("Failed to create output directory: {}", e);
            process::exit(1);
        }
    }

    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let csv_filename_raw = args.output.unwrap_or_else(|| format!("coinflip_{}.csv", timestamp));
    let csv_filename = sanitize_filename(&csv_filename_raw);
    let summary_filename = format!("summary_{}.txt", timestamp);

    let csv_path = output_dir.join(&csv_filename);
    let summary_path = output_dir.join(&summary_filename);

    let csv_mutex = if !args.no_csv {
        match File::create(&csv_path) {
            Ok(file) => {
                let mut writer = BufWriter::new(file);
                if let Err(e) = writeln!(writer, "Trial,Outcome,RandomValue") {
                    eprintln!("Failed to write CSV header: {}", e);
                    process::exit(1);
                }
                Some(Arc::new(Mutex::new(writer)))
            }
            Err(e) => {
                eprintln!("Failed to create CSV file: {}", e);
                process::exit(1);
            }
        }
    } else {
        None
    };

    let sim = CoinFlipStreamer::new(args.trials);
    sim.run_parallel(csv_mutex.clone(), args.jobs, args.bias);

    if let Err(e) = sim.export_summary(summary_path.to_str().unwrap(), args.trials, args.bias) {
        eprintln!("Failed to write summary file: {}", e);
        process::exit(1);
    }

    if let Some(writer) = csv_mutex {
        let mut writer = writer.lock().unwrap();
        if let Err(e) = writer.flush() {
            eprintln!("Failed to flush CSV: {}", e);
            process::exit(1);
        }
    }

    if !args.quiet {
        sim.print_summary_to_terminal(args.trials, args.bias, args.color, args.chart);
    }

    println!(
        "\nDone! Summary written to {}\n{}",
        summary_path.display(),
        if !args.no_csv {
            format!("CSV written to {}", csv_path.display())
        } else {
            "CSV disabled (--no-csv)".to_string()
        }
    );
}

