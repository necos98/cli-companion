//! Validazione degli input della fixture ed escaping HTML.

use serde::Deserialize;

pub const NAME_REQUIRED: &str = "Please enter your full name.";
pub const EMAIL_INVALID: &str = "Please enter a valid email address.";
pub const PASSWORD_SHORT: &str = "Password must be at least 8 characters.";
pub const PASSWORD_MISMATCH: &str = "Passwords do not match.";
pub const TERMS_REQUIRED: &str = "You must accept the terms.";
pub const DISPLAY_NAME_INVALID: &str = "Display name must be between 1 and 60 characters.";

const NAME_MAX_CHARS: usize = 100;
const EMAIL_MAX_CHARS: usize = 254;
const PASSWORD_MIN_CHARS: usize = 8;
const PASSWORD_MAX_CHARS: usize = 200;
const DISPLAY_NAME_MAX_CHARS: usize = 60;

#[derive(Deserialize, Debug)]
pub struct RegisterInput {
    pub name: String,
    pub email: String,
    pub password: String,
    pub confirm_password: String,
    pub accept_terms: bool,
}

/// Dati di registrazione gia' normalizzati (nome trimmato, email minuscola).
#[derive(Debug, PartialEq, Eq)]
pub struct Registration {
    pub name: String,
    pub email: String,
    pub password: String,
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn validate_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        return Err(NAME_REQUIRED);
    }
    Ok(name.to_string())
}

pub fn validate_email(email: &str) -> Result<String, &'static str> {
    let email = normalize_email(email);
    let well_formed = email.chars().count() <= EMAIL_MAX_CHARS
        && !email.chars().any(char::is_whitespace)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if well_formed {
        Ok(email)
    } else {
        Err(EMAIL_INVALID)
    }
}

pub fn validate_password(password: &str) -> Result<(), &'static str> {
    let chars = password.chars().count();
    if (PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&chars) {
        Ok(())
    } else {
        Err(PASSWORD_SHORT)
    }
}

pub fn validate_display_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > DISPLAY_NAME_MAX_CHARS {
        return Err(DISPLAY_NAME_INVALID);
    }
    Ok(name.to_string())
}

/// Ri-validazione lato server (autoritativa) nello stesso ordine del client.
pub fn validate_registration(input: &RegisterInput) -> Result<Registration, &'static str> {
    let name = validate_name(&input.name)?;
    let email = validate_email(&input.email)?;
    validate_password(&input.password)?;
    if input.password != input.confirm_password {
        return Err(PASSWORD_MISMATCH);
    }
    if !input.accept_terms {
        return Err(TERMS_REQUIRED);
    }
    Ok(Registration {
        name,
        email,
        password: input.password.clone(),
    })
}

pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> RegisterInput {
        RegisterInput {
            name: "  Dana Rivera ".into(),
            email: " Dana@Example.TEST ".into(),
            password: "long-enough".into(),
            confirm_password: "long-enough".into(),
            accept_terms: true,
        }
    }

    #[test]
    fn valid_registration_is_normalized() {
        let reg = validate_registration(&input()).unwrap();
        assert_eq!(reg.name, "Dana Rivera");
        assert_eq!(reg.email, "dana@example.test");
    }

    #[test]
    fn each_rule_has_its_message() {
        let mut bad = input();
        bad.name = "   ".into();
        assert_eq!(validate_registration(&bad).unwrap_err(), NAME_REQUIRED);

        let mut bad = input();
        bad.email = "no-at-sign".into();
        assert_eq!(validate_registration(&bad).unwrap_err(), EMAIL_INVALID);

        let mut bad = input();
        bad.password = "short".into();
        bad.confirm_password = "short".into();
        assert_eq!(validate_registration(&bad).unwrap_err(), PASSWORD_SHORT);

        let mut bad = input();
        bad.confirm_password = "different-pw".into();
        assert_eq!(validate_registration(&bad).unwrap_err(), PASSWORD_MISMATCH);

        let mut bad = input();
        bad.accept_terms = false;
        assert_eq!(validate_registration(&bad).unwrap_err(), TERMS_REQUIRED);
    }

    #[test]
    fn email_edge_cases() {
        for bad in ["a@b", "@b.c", "a@@b.c", "a b@c.d", "a@.c.d", "a@b.c.", ""] {
            assert!(validate_email(bad).is_err(), "{bad}");
        }
        assert_eq!(validate_email("A.B@c.dd").unwrap(), "a.b@c.dd");
    }

    #[test]
    fn display_name_limits() {
        assert_eq!(
            validate_display_name("  Morgan Lee ").unwrap(),
            "Morgan Lee"
        );
        assert!(validate_display_name("  ").is_err());
        assert!(validate_display_name(&"x".repeat(61)).is_err());
        assert!(validate_display_name(&"x".repeat(60)).is_ok());
    }

    #[test]
    fn escaping_covers_html_metacharacters() {
        assert_eq!(
            escape_html(r#"<script>"a" & 'b'</script>"#),
            "&lt;script&gt;&quot;a&quot; &amp; &#39;b&#39;&lt;/script&gt;"
        );
    }
}
