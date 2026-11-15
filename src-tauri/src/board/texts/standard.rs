//! Lo standard di ingegneria nelle sue due varianti, da un'unica fonte
//! (`prompts/engineering_standard.md`). Nella fonte, il testo che vale solo per
//! i task in coda sta fra `<queue>` e `</queue>`, quello che vale solo per una
//! sessione interattiva fra `<interactive>` e `</interactive>`; il resto vale per
//! entrambe. Una regola che resta vuota sparisce e le regole si rinumerano.

use std::sync::LazyLock;

/// Regole numerate, con i marcatori delle varianti.
const SOURCE: &str = include_str!("../prompts/engineering_standard.md");

/// Preambolo che precede le regole, per il worker e per la sessione interattiva.
const PREAMBLE: &str = include_str!("../prompts/worker_standard_preamble.md");

/// Regole dello standard per i task in coda: per il worker e per il revisore.
pub static ENGINEERING_STANDARD: LazyLock<String> = LazyLock::new(|| render_valid(Variant::Queue));

/// Standard che ogni worker riceve, prima delle regole del workspace: il
/// preambolo, una riga vuota, poi le regole.
pub static WORKER_GUARDRAILS: LazyLock<String> = LazyLock::new(|| with_preamble(&ENGINEERING_STANDARD));

/// Standard di un Bot `work` avviato in un tab: le stesse regole senza coda,
/// consegna, `complete_task` ne' `follow_ups`.
pub static INTERACTIVE_STANDARD: LazyLock<String> =
    LazyLock::new(|| with_preamble(&render_valid(Variant::Interactive)));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Queue,
    Interactive,
}

impl Variant {
    const ALL: [Variant; 2] = [Variant::Queue, Variant::Interactive];

    fn tag(self) -> &'static str {
        match self {
            Variant::Queue => "queue",
            Variant::Interactive => "interactive",
        }
    }
}

/// Un marcatore aperto e mai chiuso, o chiuso da quello dell'altra variante.
#[derive(Debug, PartialEq, Eq)]
struct MarkupError(String);

fn with_preamble(rules: &str) -> String {
    format!("{}\n\n{rules}", PREAMBLE.trim_end())
}

/// La fonte e' una costante del binario: i test garantiscono che i marcatori sono validi.
fn render_valid(variant: Variant) -> String {
    render(SOURCE, variant).expect("valid markup in engineering_standard.md")
}

/// Le righe della fonte per `variant`, rinumerate, separate da `\n` e con un `\n` finale.
fn render(source: &str, variant: Variant) -> Result<String, MarkupError> {
    let mut out = String::new();
    let mut number = 0;
    for line in source.lines() {
        let text = render_line(line, variant)?;
        if text.trim().is_empty() && !line.trim().is_empty() {
            continue;
        }
        match split_number(&text) {
            Some(rest) => {
                number += 1;
                out.push_str(&format!("{number}. {rest}"));
            }
            None => out.push_str(&text),
        }
        out.push('\n');
    }
    Ok(out)
}

/// La riga senza i marcatori: tiene il testo comune e quello di `variant`.
fn render_line(line: &str, variant: Variant) -> Result<String, MarkupError> {
    let mut out = String::new();
    let mut rest = line;
    while let Some((before, tagged)) = next_open_tag(rest) {
        out.push_str(before);
        let (owner, after_open) = tagged;
        let close = format!("</{}>", owner.tag());
        let (inner, after) =
            after_open.split_once(&close).ok_or_else(|| MarkupError(format!("unclosed <{}>", owner.tag())))?;
        if Variant::ALL.iter().any(|v| inner.contains(&format!("<{}>", v.tag()))) {
            return Err(MarkupError(format!("nested marker inside <{}>", owner.tag())));
        }
        if owner == variant {
            out.push_str(inner);
        }
        rest = after;
    }
    if Variant::ALL.iter().any(|v| rest.contains(&format!("</{}>", v.tag()))) {
        return Err(MarkupError("closing marker without its opening".to_string()));
    }
    out.push_str(rest);
    Ok(out)
}

/// Il primo marcatore di apertura di `text`: il testo prima, la variante e il testo dopo.
fn next_open_tag(text: &str) -> Option<(&str, (Variant, &str))> {
    Variant::ALL
        .iter()
        .filter_map(|&v| {
            let open = format!("<{}>", v.tag());
            text.find(&open).map(|at| (at, v, at + open.len()))
        })
        .min_by_key(|&(at, _, _)| at)
        .map(|(at, v, end)| (&text[..at], (v, &text[end..])))
}

/// Il testo di una regola numerata (`<n>. testo`) senza il suo numero.
fn split_number(line: &str) -> Option<&str> {
    let (digits, rest) = line.split_once(". ")?;
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_numbers(text: &str) -> Vec<usize> {
        text.lines().filter_map(|l| l.split_once(". ")).filter_map(|(n, _)| n.parse().ok()).collect()
    }

    #[test]
    fn no_marker_survives_in_any_variant() {
        for text in [ENGINEERING_STANDARD.as_str(), INTERACTIVE_STANDARD.as_str(), WORKER_GUARDRAILS.as_str()] {
            for v in Variant::ALL {
                assert!(!text.contains(&format!("<{}>", v.tag())) && !text.contains(&format!("</{}>", v.tag())));
            }
        }
    }

    #[test]
    fn the_queue_variant_keeps_every_rule_and_the_queue_duties() {
        assert_eq!(rule_numbers(&ENGINEERING_STANDARD), (1..=13).collect::<Vec<_>>());
        for duty in ["as follow_ups instead of fixing them", "E2E scenarios", "complete the task as failed"] {
            assert!(ENGINEERING_STANDARD.contains(duty), "missing {duty:?}");
        }
        assert!(ENGINEERING_STANDARD.contains("13. Self-review before delivering."));
        assert!(!ENGINEERING_STANDARD.contains("tell the user what still fails"));
    }

    #[test]
    fn the_interactive_variant_has_no_queue_duties() {
        let text = INTERACTIVE_STANDARD.as_str();
        for queue_only in
            ["complete_task", "follow_ups", "deliver", "Deliver", "E2E scenarios", "as failed", "specification"]
        {
            assert!(!text.contains(queue_only), "interactive standard mentions {queue_only:?}");
        }
        assert!(text.contains("Report unrelated problems you notice to the user instead of fixing them."));
        assert!(text.contains("Self-review before finishing."));
        assert!(text.contains("tell the user what still fails and why"));
    }

    #[test]
    fn the_interactive_variant_shares_the_rules_and_is_renumbered() {
        let text = INTERACTIVE_STANDARD.as_str();
        assert_eq!(rule_numbers(text), (1..=12).collect::<Vec<_>>());
        assert!(text.starts_with("Engineering standard (non-negotiable)."));
        // Le regole comuni sono identiche, parola per parola.
        let queue_rule = |prefix: &str| ENGINEERING_STANDARD.lines().find(|l| l.contains(prefix)).unwrap().to_string();
        for common in ["Security is a blocker", "Concurrency, only when", "Tests are part of the change"] {
            let rule = queue_rule(common);
            let (_, body) = rule.split_once(". ").unwrap();
            assert!(text.contains(body), "{common:?} differs");
        }
        assert!(text.contains("9. User interface, only when the change touches one."));
    }

    #[test]
    fn the_worker_guardrails_are_the_preamble_then_the_queue_rules() {
        let (preamble, rules) = WORKER_GUARDRAILS.split_once("\n\n").unwrap();
        assert_eq!(preamble, PREAMBLE.trim_end());
        assert_eq!(rules, ENGINEERING_STANDARD.as_str());
    }

    #[test]
    fn markers_pick_the_text_of_the_variant() {
        let source =
            "1. Keep <queue>Q</queue><interactive>I</interactive> end.\r\n<queue>2. Queue only.</queue>\n3. Last.\n";
        assert_eq!(render(source, Variant::Queue).unwrap(), "1. Keep Q end.\n2. Queue only.\n3. Last.\n");
        assert_eq!(render(source, Variant::Interactive).unwrap(), "1. Keep I end.\n2. Last.\n");
        assert_eq!(render("Plain\n\nText", Variant::Interactive).unwrap(), "Plain\n\nText\n");
    }

    #[test]
    fn malformed_markers_are_errors() {
        for bad in
            ["a <queue>b", "a </queue> b", "<queue>a<interactive>b</interactive></queue>", "<queue>a</interactive>"]
        {
            assert!(render(bad, Variant::Queue).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_numbered_rules_are_renumbered() {
        assert_eq!(split_number("12. Text"), Some("Text"));
        for plain in ["Text. More", ". Text", "1a. Text", "1.Text"] {
            assert_eq!(split_number(plain), None, "{plain:?}");
        }
    }
}
