//! Rendering testuale dell'osservazione della pagina per il modello.

use super::{AgentConfig, SnapshotMode};
use crate::browser::session::PageObservation;
use crate::browser::snapshot::SnapshotChange;

pub const SUPERSEDED: &str = "[observation superseded]";
const EMPTY_TREE: &str = "(no interactive elements)";
const EMPTY_TEXT: &str = "(empty)";

pub struct RenderedObservation {
    pub text: String,
    /// Solo la parte di snapshot (usata per contare i token).
    pub snapshot_section: String,
    pub snapshot_kind: &'static str,
}

/// Tiene il minimo stato per mostrare solo cio' che e' cambiato (modalita' delta).
pub struct ObservationRenderer {
    delta: bool,
    page_text_max_chars: usize,
    base_url: String,
    last_text: Option<String>,
}

impl ObservationRenderer {
    pub fn new(cfg: &AgentConfig) -> Self {
        ObservationRenderer {
            delta: cfg.snapshot_mode == SnapshotMode::Delta,
            page_text_max_chars: cfg.page_text_max_chars,
            base_url: cfg.base_url.clone(),
            last_text: None,
        }
    }

    /// `first`: prima osservazione del passo, sempre completa.
    pub fn render(&mut self, observation: &PageObservation, first: bool) -> RenderedObservation {
        let (snapshot_section, snapshot_kind) = self.snapshot_section(observation, first);
        let text_section = self.text_section(&observation.page_text, first);
        let text = format!(
            "URL: {}\nTITLE: {}\n{snapshot_section}\n{text_section}",
            relative_url(&observation.url, &self.base_url),
            observation.title,
        );
        RenderedObservation {
            text,
            snapshot_section,
            snapshot_kind,
        }
    }

    fn snapshot_section(
        &self,
        observation: &PageObservation,
        first: bool,
    ) -> (String, &'static str) {
        let view = &observation.snapshot;
        let tree = if view.tree.is_empty() {
            EMPTY_TREE
        } else {
            &view.tree
        };
        if !self.delta || first {
            return (format!("SNAPSHOT (interactive elements):\n{tree}"), "full");
        }
        match &view.change {
            SnapshotChange::Full => (format!("SNAPSHOT (page changed, full):\n{tree}"), "full"),
            SnapshotChange::Unchanged => ("SNAPSHOT: unchanged".to_string(), "unchanged"),
            SnapshotChange::Delta { removed, added } => {
                let lines = removed
                    .iter()
                    .map(|l| format!("- {l}"))
                    .chain(added.iter().map(|l| format!("+ {l}")))
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    format!("SNAPSHOT CHANGES (- removed, + added):\n{lines}"),
                    "delta",
                )
            }
        }
    }

    fn text_section(&mut self, page_text: &str, first: bool) -> String {
        let text = collapse_text(page_text, self.page_text_max_chars);
        let unchanged = self.delta && !first && self.last_text.as_deref() == Some(text.as_str());
        let section = if unchanged {
            "PAGE TEXT: unchanged".to_string()
        } else {
            let shown = if text.is_empty() { EMPTY_TEXT } else { &text };
            format!(
                "PAGE TEXT (first {} chars):\n{shown}",
                self.page_text_max_chars
            )
        };
        self.last_text = Some(text);
        section
    }
}

/// Path + query relativi all'URL base (altrimenti l'URL cosi' com'e').
pub fn relative_url(url: &str, base_url: &str) -> String {
    match url.strip_prefix(base_url) {
        Some("") => "/".to_string(),
        Some(rest) if rest.starts_with(['/', '?', '#']) => {
            if rest.starts_with('/') {
                rest.to_string()
            } else {
                format!("/{rest}")
            }
        }
        _ => url.to_string(),
    }
}

fn collapse_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ActionMode;
    use crate::browser::snapshot::SnapshotView;
    use std::collections::BTreeSet;

    const BASE: &str = "http://127.0.0.1:8790";

    fn config(delta: bool) -> AgentConfig {
        AgentConfig {
            action_mode: ActionMode::Schema,
            snapshot_mode: if delta {
                SnapshotMode::Delta
            } else {
                SnapshotMode::Full
            },
            thinking: false,
            max_actions: 15,
            temperature: 0.1,
            top_k: 50,
            repeat_penalty: 1.1,
            max_tokens: 512,
            page_text_max_chars: 20,
            base_url: BASE.to_string(),
            seed: None,
        }
    }

    fn observation(change: SnapshotChange, text: &str) -> PageObservation {
        PageObservation {
            url: format!("{BASE}/register?x=1"),
            title: "Register".to_string(),
            snapshot: SnapshotView {
                tree: "- button \"Go\" [ref=e1]".to_string(),
                refs: BTreeSet::new(),
                change,
            },
            page_text: text.to_string(),
        }
    }

    #[test]
    fn first_observation_is_always_full() {
        let mut renderer = ObservationRenderer::new(&config(true));
        let obs = observation(SnapshotChange::Unchanged, "hello   world");
        let rendered = renderer.render(&obs, true);
        assert_eq!(rendered.snapshot_kind, "full");
        assert!(rendered
            .text
            .starts_with("URL: /register?x=1\nTITLE: Register\n"));
        assert!(rendered
            .text
            .contains("PAGE TEXT (first 20 chars):\nhello world"));
    }

    #[test]
    fn delta_mode_shows_changes_then_unchanged_text() {
        let mut renderer = ObservationRenderer::new(&config(true));
        renderer.render(&observation(SnapshotChange::Full, "same"), true);
        let change = SnapshotChange::Delta {
            removed: vec!["a".into()],
            added: vec!["b".into()],
        };
        let rendered = renderer.render(&observation(change, "same"), false);
        assert_eq!(rendered.snapshot_kind, "delta");
        assert!(rendered
            .text
            .contains("SNAPSHOT CHANGES (- removed, + added):\n- a\n+ b"));
        assert!(rendered.text.contains("PAGE TEXT: unchanged"));
    }

    #[test]
    fn delta_mode_labels_full_and_unchanged() {
        let mut renderer = ObservationRenderer::new(&config(true));
        renderer.render(&observation(SnapshotChange::Full, "a"), true);
        let full = renderer.render(&observation(SnapshotChange::Full, "b"), false);
        assert!(full.text.contains("SNAPSHOT (page changed, full):"));
        let same = renderer.render(&observation(SnapshotChange::Unchanged, "b"), false);
        assert!(same.text.contains("SNAPSHOT: unchanged"));
        assert_eq!(same.snapshot_kind, "unchanged");
    }

    #[test]
    fn full_mode_ignores_change_kind_and_repeats_text() {
        let mut renderer = ObservationRenderer::new(&config(false));
        renderer.render(&observation(SnapshotChange::Full, "a"), true);
        let rendered = renderer.render(&observation(SnapshotChange::Unchanged, "a"), false);
        assert!(rendered.text.contains("SNAPSHOT (interactive elements):"));
        assert!(rendered.text.contains("PAGE TEXT (first 20 chars):\na"));
    }

    #[test]
    fn page_text_is_collapsed_and_truncated() {
        assert_eq!(collapse_text("a \n\n b\t c", 100), "a b c");
        assert_eq!(collapse_text("abcdefghij", 4), "abcd");
    }

    #[test]
    fn urls_are_made_relative_to_base() {
        assert_eq!(
            relative_url("http://127.0.0.1:8790/app?x=1", BASE),
            "/app?x=1"
        );
        assert_eq!(relative_url("http://127.0.0.1:8790", BASE), "/");
        assert_eq!(
            relative_url("http://other.test/x", BASE),
            "http://other.test/x"
        );
        assert_eq!(
            relative_url("http://127.0.0.1:87900/x", BASE),
            "http://127.0.0.1:87900/x"
        );
    }
}
