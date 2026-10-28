//! Struttura comune delle pagine: header/footer con molti link (rumore per l'agente).

use super::validate::escape_html;

/// (etichetta, percorso). I percorsi non reali servono pagine segnaposto.
type Link = (&'static str, &'static str);

const HEADER_LINKS: [Link; 25] = [
    ("Home", "/"),
    ("Pricing", "/pricing"),
    ("Features", "/features"),
    ("Docs", "/docs"),
    ("Blog", "/blog"),
    ("Careers", "/careers"),
    ("About", "/about"),
    ("Contact", "/contact"),
    ("Support", "/support"),
    ("Status", "/status"),
    ("Changelog", "/changelog"),
    ("Roadmap", "/roadmap"),
    ("Community", "/community"),
    ("Partners", "/partners"),
    ("Press", "/press"),
    ("Security", "/security"),
    ("Integrations", "/integrations"),
    ("API", "/api-reference"),
    ("Downloads", "/downloads"),
    ("Events", "/events"),
    ("Customers", "/customers"),
    ("FAQ", "/faq"),
    ("Help center", "/help"),
    ("Login", "/login"),
    ("Register", "/register"),
];

const FOOTER_LINKS: [Link; 25] = [
    ("Terms of service", "/info/terms"),
    ("Privacy policy", "/info/privacy"),
    ("Cookie policy", "/info/cookies"),
    ("Licenses", "/info/licenses"),
    ("Accessibility", "/info/accessibility"),
    ("Sitemap", "/info/sitemap"),
    ("Imprint", "/info/imprint"),
    ("Brand assets", "/info/brand"),
    ("Newsletter", "/info/newsletter"),
    ("Investors", "/info/investors"),
    ("Legal", "/info/legal"),
    ("Compliance", "/info/compliance"),
    ("Trust center", "/info/trust"),
    ("Developers", "/info/developers"),
    ("Open source", "/info/open-source"),
    ("Education", "/info/education"),
    ("Nonprofits", "/info/nonprofits"),
    ("Startups", "/info/startups"),
    ("Enterprise", "/info/enterprise"),
    ("Resellers", "/info/resellers"),
    ("Affiliates", "/info/affiliates"),
    ("Media kit", "/info/media-kit"),
    ("Webinars", "/info/webinars"),
    ("Case studies", "/info/case-studies"),
    ("Contact sales", "/info/sales"),
];

/// Percorsi che hanno una pagina reale: tutti gli altri link sono segnaposto.
const REAL_PATHS: [&str; 3] = ["/", "/login", "/register"];

/// Titolo della pagina segnaposto per `path`, se e' un link noto.
pub fn placeholder_title(path: &str) -> Option<&'static str> {
    if REAL_PATHS.contains(&path) {
        return None;
    }
    HEADER_LINKS
        .iter()
        .chain(FOOTER_LINKS.iter())
        .find(|(_, href)| *href == path)
        .map(|(label, _)| *label)
}

fn links_html(links: &[Link]) -> String {
    let items: String = links
        .iter()
        .map(|(label, href)| format!("<li><a href=\"{href}\">{label}</a></li>"))
        .collect();
    format!("<nav><ul>{items}</ul></nav>")
}

fn shell(title: &str, header: &str, main: &str, scripts: &[&str]) -> String {
    let scripts: String = scripts
        .iter()
        .map(|src| format!("<script src=\"/assets/{src}\" defer></script>"))
        .collect();
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<link rel=\"stylesheet\" href=\"/assets/style.css\">\n</head>\n\
         <body>\n<header>{header}</header>\n<main>\n{main}\n</main>\n\
         <footer>{}</footer>\n{scripts}\n</body>\n</html>\n",
        escape_html(title),
        links_html(&FOOTER_LINKS)
    )
}

/// Pagina pubblica con l'header a 25 link. `main` e' HTML gia' sicuro.
pub fn public_page(title: &str, main: &str, scripts: &[&str]) -> String {
    shell(title, &links_html(&HEADER_LINKS), main, scripts)
}

/// Pagina dell'area autenticata: header con utente e navigazione.
pub fn app_page(title: &str, display_name: &str, main: &str, scripts: &[&str]) -> String {
    let header = format!(
        "<div class=\"app-header\"><span id=\"signed-in-as\">Signed in as {}</span>\
         <nav><a href=\"/app\">Dashboard</a><a href=\"/app/settings\">Settings</a>\
         <button id=\"signout\" type=\"button\">Sign out</button></nav></div>",
        escape_html(display_name)
    );
    shell(title, &header, main, scripts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_titles_resolve_for_known_links_only() {
        assert_eq!(placeholder_title("/pricing"), Some("Pricing"));
        assert_eq!(placeholder_title("/info/terms"), Some("Terms of service"));
        assert_eq!(placeholder_title("/login"), None);
        assert_eq!(placeholder_title("/unknown"), None);
    }

    #[test]
    fn all_link_targets_are_unique() {
        let mut hrefs: Vec<_> = HEADER_LINKS
            .iter()
            .chain(FOOTER_LINKS.iter())
            .map(|l| l.1)
            .collect();
        hrefs.sort_unstable();
        hrefs.dedup();
        assert_eq!(hrefs.len(), 50);
    }

    #[test]
    fn titles_and_names_are_escaped() {
        let page = app_page("<x>", "<b>Dana</b>", "", &[]);
        assert!(page.contains("<title>&lt;x&gt;</title>"));
        assert!(page.contains("Signed in as &lt;b&gt;Dana&lt;/b&gt;"));
        assert!(!page.contains("<b>Dana"));
    }

    #[test]
    fn public_page_has_header_and_footer_links() {
        let page = public_page("T", "<p>m</p>", &["common.js"]);
        assert_eq!(page.matches("<li>").count(), 50);
        assert!(page.contains("/assets/common.js"));
    }
}
