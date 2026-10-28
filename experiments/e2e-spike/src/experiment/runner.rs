//! Esecuzione di una configurazione: server, campionatore VRAM e tutti gli scenari.

use super::config::RunConfig;
use super::fixture_client::FixtureClient;
use super::output::{Output, Sink};
use super::scenario_run::{execute, ScenarioRun};
use crate::llama::client::LlamaClient;
use crate::llama::server::{LlamaServer, LlamaServerConfig};
use crate::metrics::gpu::VramSampler;
use crate::metrics::summary::{ConfigData, ResourceUsage};
use crate::paths::Paths;
use crate::scenario::Scenario;
use std::path::{Path, PathBuf};
use std::time::Duration;

const VRAM_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// Tutto cio' che e' comune alle configurazioni di una run.
pub struct Env<'a> {
    pub paths: &'a Paths,
    pub fixture: &'a FixtureClient,
    pub fixture_url: &'a str,
    pub scenarios: &'a [Scenario],
    pub runs: u32,
    pub llama_port: u16,
    pub headed: bool,
    pub out_dir: &'a Path,
}

/// Esegue la configurazione riempiendo `data`; un errore lascia in `data` i risultati parziali.
pub fn run_config(
    env: &Env,
    config: &RunConfig,
    output: &mut Output,
    data: &mut ConfigData,
) -> Result<(), String> {
    println!("== Configurazione {}: avvio di llama-server", config.id);
    let sampler = VramSampler::start(VRAM_SAMPLE_INTERVAL);
    let result = match start_server(env, config) {
        Ok(server) => {
            let result = run_scenarios(env, config, &server, output, data);
            data.resources.peak_ram_mib =
                ResourceUsage::peak_ram_from_bytes(server.peak_working_set_bytes());
            if let Err(e) = server.stop() {
                eprintln!("avviso: arresto di llama-server: {e}");
            }
            result
        }
        Err(e) => {
            println!("   non eseguibile: {e}");
            data.not_runnable = Some(e);
            Ok(())
        }
    };
    data.resources.vram = sampler.stop();
    result
}

fn start_server(env: &Env, config: &RunConfig) -> Result<LlamaServer, String> {
    let model = env.paths.model(config.quant);
    if !model.exists() {
        return Err(format!(
            "modello mancante: {} (eseguire `setup`)",
            model.display()
        ));
    }
    let log_file: PathBuf = env.out_dir.join(format!("llama-{}.log", config.id));
    let server_config = LlamaServerConfig::new(
        env.paths.llama_server_exe(),
        model,
        env.llama_port,
        log_file,
    );
    LlamaServer::start(&server_config)
}

fn run_scenarios(
    env: &Env,
    config: &RunConfig,
    server: &LlamaServer,
    output: &mut Output,
    data: &mut ConfigData,
) -> Result<(), String> {
    let llm = LlamaClient::new(server.base_url());
    let browser_exe = env.paths.agent_browser_exe();
    let mut sink = Sink { output, data };
    for scenario in env.scenarios {
        for run in 1..=env.runs {
            let job = ScenarioRun {
                config,
                scenario,
                run,
                llm: &llm,
                fixture: env.fixture,
                fixture_url: env.fixture_url,
                browser_exe: &browser_exe,
                headed: env.headed,
            };
            execute(&job, &mut sink)?;
        }
    }
    Ok(())
}
