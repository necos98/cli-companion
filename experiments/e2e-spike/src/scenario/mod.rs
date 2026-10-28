//! Scenari di test in linguaggio naturale e verdetto deterministico.

mod definitions;
pub mod test_data;
pub mod verdict;

pub use definitions::all;
pub use verdict::Check;

/// Stato da preparare sul fixture prima dell'esecuzione.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seed {
    Nothing,
    /// Un account con nome, email e password dei dati di test.
    TestAccount,
}

#[derive(Debug)]
pub struct ScenarioStep {
    pub text: &'static str,
    pub checks: Vec<Check>,
    /// Il passo chiede di riferire un esito: il flag `success` di `done` e' solo registrato.
    pub is_report: bool,
}

#[derive(Debug)]
pub struct Scenario {
    pub id: &'static str,
    pub seed: Seed,
    /// Dati di test aggiuntivi oltre a name/email/password.
    pub extras: &'static [(&'static str, &'static str)],
    pub steps: Vec<ScenarioStep>,
}
