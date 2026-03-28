use clap::Parser;
use std::io::{self, Read};

mod decoder;
mod encoder;
use encoder::{Delimiter, Encoder};

#[derive(Parser)]
#[command(name = "jtoon", about = "Convert between JSON and TOON formats")]
struct Cli {
    /// Input file (reads from stdin if not provided)
    input: Option<String>,

    /// Output file (writes to stdout if not provided)
    #[arg(short, long)]
    output: Option<String>,

    /// Decode TOON to JSON (default: encode JSON to TOON)
    #[arg(short, long)]
    decode: bool,

    /// Compact JSON output (only with --decode)
    #[arg(short, long)]
    compact: bool,

    /// Indentation size
    #[arg(long, default_value = "2")]
    indent: usize,

    /// Delimiter: comma, tab, or pipe
    #[arg(long, default_value = "comma")]
    delimiter: String,
}

fn main() {
    let cli = Cli::parse();
    let input = read_input(&cli);

    if cli.decode {
        let value = decoder::decode(&input).unwrap_or_else(|e| {
            eprintln!("jtoon: decode error: {}", e);
            std::process::exit(1);
        });
        let json = if cli.compact {
            serde_json::to_string(&value)
        } else {
            serde_json::to_string_pretty(&value)
        }
        .unwrap();
        println!("{}", json);
    } else {
        let value: serde_json::Value = serde_json::from_str(&input).unwrap_or_else(|e| {
            eprintln!("jtoon: invalid JSON: {}", e);
            std::process::exit(1);
        });
        let delimiter = match cli.delimiter.as_str() {
            "comma" => Delimiter::Comma,
            "tab" => Delimiter::Tab,
            "pipe" => Delimiter::Pipe,
            other => {
                eprintln!(
                    "jtoon: invalid delimiter '{}' (use: comma, tab, pipe)",
                    other
                );
                std::process::exit(1);
            }
        };
        let enc = Encoder::new(cli.indent, delimiter);
        let toon = enc.encode(&value);

        match &cli.output {
            Some(path) => {
                std::fs::write(path, &toon).unwrap_or_else(|e| {
                    eprintln!("jtoon: error writing '{}': {}", path, e);
                    std::process::exit(1);
                });
            }
            None => {
                if !toon.is_empty() {
                    println!("{}", toon);
                }
            }
        }
    }
}

fn read_input(cli: &Cli) -> String {
    match &cli.input {
        Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("jtoon: error reading '{}': {}", path, e);
            std::process::exit(1);
        }),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                eprintln!("jtoon: error reading stdin: {}", e);
                std::process::exit(1);
            });
            buf
        }
    }
}
