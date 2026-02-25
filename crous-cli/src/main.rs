use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "crous", version, about = "Crous binary format CLI tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect a .crous file and show block layout
    Inspect {
        /// Path to the .crous file
        file: PathBuf,
    },
    /// Pretty-print a .crous file in human-readable text notation
    Pretty {
        /// Path to the .crous file
        file: PathBuf,
        /// Indentation width (default: 4)
        #[arg(short, long, default_value_t = 4)]
        indent: usize,
    },
    /// Convert a .crous file to JSON
    ToJson {
        /// Path to the .crous file
        file: PathBuf,
        /// Pretty-print JSON (default: true)
        #[arg(short, long, default_value_t = true)]
        pretty: bool,
    },
    /// Convert a JSON file to .crous binary
    FromJson {
        /// Path to the JSON file
        file: PathBuf,
        /// Output path for the .crous file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Parse a .crous text file and encode to binary
    Encode {
        /// Path to the .crous text file
        file: PathBuf,
        /// Output path for the binary .crous file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Quick benchmark encoding/decoding of a file
    Bench {
        /// Path to a JSON file to benchmark
        file: PathBuf,
        /// Number of iterations
        #[arg(short = 'n', long, default_value_t = 1000)]
        iterations: usize,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { file } => cmd_inspect(&file),
        Commands::Pretty { file, indent } => cmd_pretty(&file, indent),
        Commands::ToJson { file, pretty } => cmd_to_json(&file, pretty),
        Commands::FromJson { file, output } => cmd_from_json(&file, output),
        Commands::Encode { file, output } => cmd_encode(&file, output),
        Commands::Bench { file, iterations } => cmd_bench(&file, iterations),
    }
}

fn cmd_inspect(path: &PathBuf) {
    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    });

    // Parse header.
    match crous_core::header::FileHeader::decode(&data) {
        Ok(header) => {
            println!("=== Crous File: {} ===", path.display());
            println!("File size: {} bytes", data.len());
            println!("Header flags: 0x{:02x}", header.flags);
            println!("  Has index:  {}", header.has_index());
            println!("  Has schema: {}", header.has_schema());
            println!();
        }
        Err(e) => {
            eprintln!("Invalid Crous file: {e}");
            std::process::exit(1);
        }
    }

    // Walk blocks.
    let mut offset = crous_core::header::HEADER_SIZE;
    let mut block_num = 0u32;
    while offset < data.len() {
        match crous_core::block::BlockReader::parse(&data, offset) {
            Ok((block, consumed)) => {
                println!(
                    "Block #{block_num}: type={:?}, comp={:?}, payload={} bytes, checksum=0x{:016x}, valid={}",
                    block.block_type,
                    block.compression,
                    block.payload.len(),
                    block.checksum,
                    block.verify_checksum()
                );
                offset += consumed;
                block_num += 1;
            }
            Err(e) => {
                eprintln!("Error at offset {offset}: {e}");
                break;
            }
        }
    }
    println!("\nTotal blocks: {block_num}");
}

fn cmd_pretty(path: &PathBuf, indent: usize) {
    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    });

    let mut decoder = crous_core::Decoder::new(&data);
    match decoder.decode_all_owned() {
        Ok(values) => {
            for value in &values {
                println!("{}", crous_core::text::pretty_print(value, indent));
            }
        }
        Err(e) => {
            eprintln!("Decode error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_to_json(path: &PathBuf, pretty: bool) {
    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    });

    let mut decoder = crous_core::Decoder::new(&data);
    match decoder.decode_all_owned() {
        Ok(values) => {
            let json_values: Vec<serde_json::Value> =
                values.iter().map(serde_json::Value::from).collect();
            let output = if json_values.len() == 1 {
                if pretty {
                    serde_json::to_string_pretty(&json_values[0]).unwrap()
                } else {
                    serde_json::to_string(&json_values[0]).unwrap()
                }
            } else if pretty {
                serde_json::to_string_pretty(&json_values).unwrap()
            } else {
                serde_json::to_string(&json_values).unwrap()
            };
            println!("{output}");
        }
        Err(e) => {
            eprintln!("Decode error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_from_json(path: &PathBuf, output: Option<PathBuf>) {
    let json_str = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    });

    let json_value: serde_json::Value = serde_json::from_str(&json_str).unwrap_or_else(|e| {
        eprintln!("Invalid JSON: {e}");
        std::process::exit(1);
    });

    let crous_value = crous_core::Value::from(&json_value);
    let mut encoder = crous_core::Encoder::new();
    encoder.encode_value(&crous_value).unwrap();
    let bytes = encoder.finish().unwrap();

    let out_path = output.unwrap_or_else(|| path.with_extension("crous"));
    fs::write(&out_path, &bytes).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {e}", out_path.display());
        std::process::exit(1);
    });

    println!(
        "Wrote {} bytes to {} (JSON was {} bytes, {:.1}% reduction)",
        bytes.len(),
        out_path.display(),
        json_str.len(),
        (1.0 - bytes.len() as f64 / json_str.len() as f64) * 100.0
    );
}

fn cmd_encode(path: &PathBuf, output: Option<PathBuf>) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    });

    let value = crous_core::text::parse(&text).unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        std::process::exit(1);
    });

    let mut encoder = crous_core::Encoder::new();
    encoder.encode_value(&value).unwrap();
    let bytes = encoder.finish().unwrap();

    let out_path = output.unwrap_or_else(|| path.with_extension("crous"));
    fs::write(&out_path, &bytes).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {e}", out_path.display());
        std::process::exit(1);
    });
    println!("Wrote {} bytes to {}", bytes.len(), out_path.display());
}

fn cmd_bench(path: &PathBuf, iterations: usize) {
    let json_str = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    });

    let json_value: serde_json::Value = serde_json::from_str(&json_str).unwrap_or_else(|e| {
        eprintln!("Invalid JSON: {e}");
        std::process::exit(1);
    });

    let crous_value = crous_core::Value::from(&json_value);

    // Encode benchmark.
    let start = std::time::Instant::now();
    let mut last_bytes = Vec::new();
    for _ in 0..iterations {
        let mut encoder = crous_core::Encoder::new();
        encoder.encode_value(&crous_value).unwrap();
        last_bytes = encoder.finish().unwrap();
    }
    let encode_time = start.elapsed();

    println!("=== Benchmark: {} ===", path.display());
    println!("Iterations: {iterations}");
    println!("JSON size:  {} bytes", json_str.len());
    println!(
        "Crous size: {} bytes ({:.1}% of JSON)",
        last_bytes.len(),
        last_bytes.len() as f64 / json_str.len() as f64 * 100.0
    );
    println!(
        "Encode: {:.2?} total, {:.2?}/iter, {:.1} MB/s",
        encode_time,
        encode_time / iterations as u32,
        (json_str.len() * iterations) as f64 / encode_time.as_secs_f64() / 1_000_000.0
    );

    // Decode benchmark.
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let mut decoder = crous_core::Decoder::new(&last_bytes);
        let _ = decoder.decode_all_owned().unwrap();
    }
    let decode_time = start.elapsed();

    println!(
        "Decode: {:.2?} total, {:.2?}/iter, {:.1} MB/s",
        decode_time,
        decode_time / iterations as u32,
        (last_bytes.len() * iterations) as f64 / decode_time.as_secs_f64() / 1_000_000.0
    );
}
