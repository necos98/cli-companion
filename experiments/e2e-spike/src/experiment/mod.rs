//! Orchestrazione dell'esperimento: matrice di configurazioni x scenari x ripetizioni.

mod config;
mod fixture_client;
mod meta;
mod output;
mod runner;
mod scenario_run;
mod selection;

use crate::fixture::FixtureServer;
use crate::metrics::summary::ConfigData;
use crate::paths::Paths;
use fixture_client::FixtureClient;
use meta::Meta;
use output::Output;
use runner::{run_config, Env};
use std::path::PathBuf;

const OUT_DIR_FORMAT: &str = "%Y%m%d-%H%M%S";

#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    /// Configurazioni da eseguire, separate da virgola (default: tutte).
    #[arg(long, value_delimiter = ',')]
    pub configs: Vec<String>,
    /// Scenari da eseguire, separati da virgola (default: tutti).
    #[arg(long, value_delimiter = ',')]
    pub scenarios: Vec<String>,
    /// Ripetizioni per ogni scenario.
    #[arg(long, default_value_t = 5)]
    pub runs: u32,
    /// Azioni massime per passo (sovrascrive il valore delle configurazioni).
    #[arg(long)]
    pub max_actions: Option<u32>,
    /// Porta di llama-server.
    #[arg(long, default_value_t = 18080)]
    pub llama_port: u16,
    /// Porta del sito di prova (0 = effimera).
    #[arg(long, default_value_t = 0)]
    pub fixture_port: u16,
    /// Cartella dei risultati (default: results/<data-ora>).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Mostra la finestra del browser.
    #[arg(long)]
    pub headed: bool,
}

pub fn run(paths: &Paths, args: RunArgs) -> Result<(), String> {
    if args.runs == 0 {
        return Err("--runs deve essere almeno 1".to_string());
    }
    let mut configs = selection::select_configs(&args.configs)?;
    if let Some(max_actions) = args.max_actions {
        configs
            .iter_mut()
            .for_each(|c| c.max_actions_per_step = max_actions);
    }
    let scenarios = selection::select_scenarios(&args.scenarios)?;
    let started_at = chrono::Local::now();
    let out_dir = args.out.clone().unwrap_or_else(|| {
        paths
            .results
            .join(started_at.format(OUT_DIR_FORMAT).to_string())
    });
    let meta = Meta::collect(paths, &configs, started_at.to_rfc3339())?;
    let mut output = Output::create(&out_dir)?;
    output.write_meta(&meta.to_json(&args, None))?;
    println!("Risultati in {}", output.dir().display());

    let fixture = FixtureServer::start(args.fixture_port)?;
    let client = FixtureClient::new(fixture.base_url());
    let env = Env {
        paths,
        fixture: &client,
        fixture_url: fixture.base_url(),
        scenarios: &scenarios,
        runs: args.runs,
        llama_port: args.llama_port,
        headed: args.headed,
        out_dir: &out_dir,
    };
    let result = run_all(&env, &configs, &mut output);
    fixture.stop();
    let finished = chrono::Local::now().to_rfc3339();
    output.write_meta(&meta.to_json(&args, Some(finished)))?;
    result
}

/// Esegue le configurazioni in ordine, riscrivendo il riepilogo dopo ciascuna.
fn run_all(env: &Env, configs: &[config::RunConfig], output: &mut Output) -> Result<(), String> {
    let mut all: Vec<ConfigData> = Vec::new();
    for config in configs {
        all.push(ConfigData::new(config.id));
        let data = all.last_mut().ok_or("nessun dato di configurazione")?;
        let result = run_config(env, config, output, data);
        output.write_summary(&all)?;
        result?;
    }
    println!(
        "Riepilogo scritto in {}",
        output.dir().join("summary.md").display()
    );
    Ok(())
}
