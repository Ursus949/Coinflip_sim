use std::process;
use std::fs::{self, File};
use std::path::PathBuf;
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex};

use clap::Parser;
use chrono::Local;
use indicatif::{ProgressBar, ProgressStyle};
use log::info;
use rand::random_range;
use rayon::prelude::*;

#[derive(Parser, Debug)]
#[command(name = "Coin Flip Simulator")]
#[command(version = "3.1", author = "Ursus")]
#[command(about = "Simulates coin flips in parallel and streams results directly to CSV (memory efficient)")]
struct Args {
    /// Number of coin flip trials to perform
    #[arg(short, long)]
    trials: u64,

    /// Output CSV filename (placed in ./output/)
    #[arg(short, long)]
    output: Option<String>,

    /// Disable CSV output entirely
    #[arg(long)]
    no_csv: bool,

    /// Number of parallel threads (default: # of cores)
    #[arg(short = 'j', long)]
    jobs: Option<usize>,
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

    fn run_parallel(&self, csv_mutex: Option<Arc<Mutex<BufWriter<File>>>>, num_threads: Option<usize>) {
        info!("Starting simulation with {} trials", self.trials);

        if let Some(jobs) = num_threads {
            rayon::ThreadPoolBuilder::new()
                .num_threads(jobs)
                .build_global()
                .unwrap_or_else(|e| {
                    eprintln!("Failed to set thread pool: {}", e);
                    process::exit(1);
                });
        }

        let pb = ProgressBar::new(self.trials);
        pb.set_style(ProgressStyle::with_template("[{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent}% {msg}")
            .unwrap()
            .progress_chars("=>-"));

        (1..=self.trials).into_par_iter().for_each(|trial| {
            let value = random_range(1..=100);
            let (outcome_str, is_head) = if value <= 50 {
                ("H".to_string(), true)
            } else {
                ("T".to_string(), false)
            };

            if let Some(ref file_mutex) = csv_mutex {
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
        });

        pb.finish_with_message("Done");
        info!("Simulation complete");
    }

    fn export_summary(&self, filename: &str, total: u64) -> Result<(), Box<dyn std::error::Error>> {
        let heads = *self.head_count.lock().unwrap();
        let tails = *self.tail_count.lock().unwrap();
        let head_pct = 100.0 * heads as f64 / total as f64;
        let tail_pct = 100.0 * tails as f64 / total as f64;

        info!("Writing summary to {}", filename);
        let mut file = File::create(filename)?;
        writeln!(file, "Summary Report")?;
        writeln!(file, "==============")?;
        writeln!(file, "Total Trials: {}", total)?;
        writeln!(file, "Heads: {} ({:.2}%)", heads, head_pct)?;
        writeln!(file, "Tails: {} ({:.2}%)", tails, tail_pct)?;
        info!("Summary report written.");
        Ok(())
    }
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    if args.trials == 0 {
        eprintln!("Please provide a positive number of trials.");
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
    let csv_filename = args.output.unwrap_or_else(|| format!("coinflip_{}.csv", timestamp));
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
    sim.run_parallel(csv_mutex.clone(), args.jobs);

    if let Err(e) = sim.export_summary(summary_path.to_str().unwrap(), args.trials) {
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

    println!(
        "Done! Summary written to {}
{}",
        summary_path.display(),
        if !args.no_csv {
            format!("CSV written to {}", csv_path.display())
        } else {
            "CSV disabled (--no-csv)".to_string()
        }
    );
}
