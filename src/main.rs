use anyhow::Result;

use crate::cli::Opts;

pub mod cli;

fn main() {
    if let Err(err) = run() {
        eprintln!("{}", err);
        std::process::exit(0x0100)
    }
}

fn run() -> Result<()> {
    let args = Opts::parse_args()?;
    println!("args: {:?}", args);

    Ok(())
}
