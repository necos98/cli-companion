//! e2e-spike: esperimento LFM3-1.7B + llama-server + agent-browser per test E2E
//! scritti in linguaggio naturale.

mod agent;
mod artifacts;
mod browser;
mod experiment;
mod fixture;
mod hash;
mod llama;
mod metrics;
mod paths;
mod proc;
mod scenario;
mod setup;

use clap::{Parser, Subcommand};
use paths::Paths;
use std::process::ExitCode;

const DEFAULT_FIXTURE_PORT: u16 = 8790;

#[derive(Parser)]
#[command(name = "e2e-spike", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scarica e verifica llama.cpp, i modelli GGUF e agent-browser
    Setup,
    /// Avvia il sito di prova per provarlo a mano
    ServeFixture {
        #[arg(long, default_value_t = DEFAULT_FIXTURE_PORT)]
        port: u16,
    },
    /// Esegue l'esperimento
    Run(experiment::RunArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = Paths::from_manifest();
    let result = match cli.command {
        Command::Setup => setup::run(&paths),
        Command::ServeFixture { port } => fixture::serve_forever(port),
        Command::Run(args) => experiment::run(&paths, args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Errore: {e}");
            ExitCode::FAILURE
        }
    }
}
