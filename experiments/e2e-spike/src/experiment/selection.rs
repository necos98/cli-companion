//! Selezione di configurazioni e scenari richiesti da riga di comando.

use super::config::{matrix, RunConfig};
use crate::scenario::{self, Scenario};

pub fn select_configs(requested: &[String]) -> Result<Vec<RunConfig>, String> {
    select("configurazione", requested, matrix(), |c| c.id)
}

pub fn select_scenarios(requested: &[String]) -> Result<Vec<Scenario>, String> {
    select("scenario", requested, scenario::all(), |s| s.id)
}

/// Filtra `all` mantenendone l'ordine; nessun filtro richiesto = tutti.
fn select<T>(
    what: &str,
    requested: &[String],
    all: Vec<T>,
    id_of: impl Fn(&T) -> &'static str,
) -> Result<Vec<T>, String> {
    let valid: Vec<&'static str> = all.iter().map(&id_of).collect();
    if let Some(unknown) = requested
        .iter()
        .find(|r| !valid.iter().any(|v| v.eq_ignore_ascii_case(r)))
    {
        return Err(format!(
            "{what} sconosciuta/o `{unknown}`; valori validi: {}",
            valid.join(", ")
        ));
    }
    if requested.is_empty() {
        return Ok(all);
    }
    Ok(all
        .into_iter()
        .filter(|item| {
            requested
                .iter()
                .any(|r| r.eq_ignore_ascii_case(id_of(item)))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_request_selects_everything() {
        assert_eq!(select_configs(&[]).unwrap().len(), 5);
        assert_eq!(select_scenarios(&[]).unwrap().len(), 4);
    }

    #[test]
    fn selection_keeps_matrix_order_and_ignores_case() {
        let ids: Vec<&str> = select_configs(&strings(&["delta", "BASE"]))
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["base", "delta"]);
    }

    #[test]
    fn unknown_id_lists_valid_ones() {
        let error = select_scenarios(&strings(&["S9"])).unwrap_err();
        assert!(error.contains("`S9`"));
        assert!(error.contains("S1, S1-it, S2, S3"));
    }
}
