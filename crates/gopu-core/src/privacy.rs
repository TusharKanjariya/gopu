//! What memory never captures, and the masks applied before anything is stored (memory-v1.1 ticket 03).
//! Built in and offline. Words follow GLOSSARY.md: never-capture list, mask.

use regex::Regex;
use std::sync::OnceLock;

/// Bump when the masks change: rows stored under an older version get masked again.
pub const MASK_VERSION: i64 = 2;

/// Apps never captured (exe name, lowercase): password managers and Windows' own sign-in and lock screens.
const APPS: &[&str] = &[
    "keepass.exe", "keepassxc.exe", "1password.exe", "bitwarden.exe", "lastpass.exe", "dashlane.exe", "enpass.exe",
    "nordpass.exe", "proton pass.exe", "lockapp.exe", "logonui.exe", "credentialuibroker.exe", "consent.exe",
];

/// Private and incognito browser windows, by window title.
// ponytail: English titles only; translated titles slip through. Add locales when a user hits one.
const PRIVATE_TITLES: &[&str] = &["(incognito)", "inprivate", "private browsing", "(private)", "- private window"];

/// Sites never captured: the domain and all its subdomains. A short seed for v1.1: sign-in hosts, web password
/// managers, banks and payments, health, adult.
const DOMAINS: &[&str] = &[
    // sign-in and password managers
    "accounts.google.com", "login.microsoftonline.com", "login.live.com", "appleid.apple.com", "vault.bitwarden.com",
    "my.1password.com", "lastpass.com",
    // banks, payments, investing
    "onlinesbi.sbi", "sbi.co.in", "hdfcbank.com", "icicibank.com", "axisbank.com", "kotak.com", "pnbindia.in",
    "bankofbaroda.in", "yesbank.in", "idfcfirstbank.com", "paytm.com", "phonepe.com", "zerodha.com", "groww.in",
    "paypal.com", "chase.com", "bankofamerica.com", "wellsfargo.com", "citi.com", "hsbc.com", "barclays.co.uk",
    "revolut.com", "wise.com",
    // health
    "practo.com", "1mg.com", "pharmeasy.in", "apollo247.com", "netmeds.com", "mychart.com",
    // adult
    "pornhub.com", "xvideos.com", "xnxx.com", "xhamster.com", "redtube.com", "youporn.com", "onlyfans.com",
];

/// Sign-in, password and payment pages on any site, by one whole path segment.
const PATHS: &[&str] = &[
    "login", "signin", "sign-in", "logon", "auth", "oauth", "oauth2", "sso", "password", "passwords",
    "reset-password", "forgot-password", "2fa", "mfa", "checkout", "payment", "payments", "wallet",
];

/// Browser pages that show saved passwords and cards.
const URL_PREFIXES: &[&str] = &[
    "chrome://password-manager", "chrome://settings/passwords", "chrome://settings/payments", "edge://wallet",
    "edge://settings/passwords", "edge://settings/payments", "brave://password-manager", "about:logins",
];

/// A browser address bar's text as the site memory keeps: host + path, no scheme, query string or #fragment.
/// None when it isn't an address (search words typed into the bar).
pub fn site_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.contains(char::is_whitespace) {
        return None;
    }
    let url = url.split(['?', '#']).next().unwrap_or_default();
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    let url = url.trim_end_matches('/');
    let (host, path) = url.split_once('/').unwrap_or((url, ""));
    let internal = host.contains(':'); // chrome://…, about:logins
    if !internal && !host.contains('.') && host != "localhost" {
        return None;
    }
    Some(if path.is_empty() { host.to_lowercase() } else { format!("{}/{path}", host.to_lowercase()) })
}

/// True when memory must leave this window alone: no text, no title, no row at all.
pub fn never_capture(app: &str, title: &str, site: Option<&str>) -> bool {
    let app = app.to_lowercase();
    let title = title.to_lowercase();
    if APPS.contains(&app.as_str()) || PRIVATE_TITLES.iter().any(|t| title.contains(t)) {
        return true;
    }
    let Some(site) = site else { return false };
    let site = site.to_lowercase();
    if URL_PREFIXES.iter().any(|p| site.starts_with(p)) {
        return true;
    }
    let (host, path) = site.split_once('/').unwrap_or((&site, ""));
    DOMAINS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
        || path.split('/').any(|seg| PATHS.contains(&seg))
}

/// An ignore-list entry as typed ("Signal", "https://web.whatsapp.com/x") made canonical: an app as its exe name
/// ("signal.exe"), a site as its host ("web.whatsapp.com"). None for blank input.
pub fn ignore_entry(input: &str) -> Option<String> {
    let s = input.trim().to_lowercase();
    if s.is_empty() || s.ends_with(".exe") {
        return Some(s).filter(|s| !s.is_empty());
    }
    Some(match site_from_url(&s) {
        Some(site) => site.split('/').next().unwrap_or_default().to_string(),
        None => format!("{s}.exe"), // "Signal", "Proton Pass"
    })
}

/// Does an ignore-list entry cover this window? Apps by exe name; a site covers its subdomains.
pub fn ignored(entry: &str, app: &str, site: Option<&str>) -> bool {
    if entry.ends_with(".exe") {
        return app.eq_ignore_ascii_case(entry);
    }
    let host = site.unwrap_or_default().split('/').next().unwrap_or_default().to_lowercase();
    !host.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
}

/// `text` with secrets replaced by placeholders: [key], [secret], [iban], [card], [aadhaar], [pan].
/// Number-shaped masks need their checksum to pass, so ordinary numbers stay readable.
pub fn mask(text: &str) -> String {
    type Check = fn(&str) -> bool;
    static MASKS: OnceLock<Vec<(Regex, &'static str, Option<Check>)>> = OnceLock::new();
    let masks = MASKS.get_or_init(|| {
        let r = |p: &str| Regex::new(p).expect("mask pattern");
        vec![
            (r(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"), "[key]", None),
            (
                r(concat!(
                    r"\b(?:AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,}|gsk_[A-Za-z0-9]{20,}",
                    r"|sk-ant-[A-Za-z0-9_-]{20,}|sk-(?:proj-)?[A-Za-z0-9_-]{20,}|xox[abposr]-[A-Za-z0-9-]{10,}",
                    r"|AIza[0-9A-Za-z_-]{35}|(?:sk|rk|pk)_live_[0-9A-Za-z]{16,}",
                    r"|eyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})",
                )),
                "[key]",
                None,
            ),
            (
                r(r#"(?i)\b(password|passwd|pwd|passcode|secret|api[_-]?key|access[_-]?token|auth[_-]?token)(\s*[:=]\s*)("[^"]*"|'[^']*'|\S+)"#),
                "${1}${2}[secret]",
                None,
            ),
            // a link's query values that let anyone in: friend links (Medium's sk), sign-in codes, signed URLs
            (
                r(r"(?i)([?&](?:sk|code|code_challenge|code_verifier|state|token|id_token|access_token|refresh_token|sig|signature|key|client_secret|otp|auth|x-amz-signature|x-amz-credential)=)[^&\s#\]]+"),
                "${1}[secret]",
                None,
            ),
            (r(r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]){11,30}\b"), "[iban]", Some(iban_ok)),
            (r(r"\b\d(?:[ -]?\d){12,18}\b"), "[card]", Some(luhn_ok)),
            (r(r"\b[2-9]\d{3}[ -]?\d{4}[ -]?\d{4}\b"), "[aadhaar]", Some(verhoeff_ok)),
            (r(r"\b[A-Z]{3}[PCHFATBLJG][A-Z]\d{4}[A-Z]\b"), "[pan]", None),
        ]
    });
    let mut out = text.to_string();
    for (re, with, check) in masks {
        out = match check {
            None => re.replace_all(&out, *with).into_owned(),
            Some(ok) => re.replace_all(&out, |c: &regex::Captures| if ok(&c[0]) { with.to_string() } else { c[0].to_string() }).into_owned(),
        };
    }
    out
}

fn digits(s: &str) -> Vec<u32> {
    s.chars().filter_map(|c| c.to_digit(10)).collect()
}

/// Card numbers: the Luhn checksum.
fn luhn_ok(s: &str) -> bool {
    let d = digits(s);
    let sum: u32 = d.iter().rev().enumerate().map(|(i, &x)| if i % 2 == 1 { if x * 2 > 9 { x * 2 - 9 } else { x * 2 } } else { x }).sum();
    (13..=19).contains(&d.len()) && sum % 10 == 0
}

/// IBANs: country code, check digits, then mod 97 of the rearranged number is 1.
fn iban_ok(s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut rem = 0u32;
    for c in rearranged.chars() {
        let v = match c.to_digit(36) {
            Some(v) => v,
            None => return false,
        };
        rem = if v < 10 { (rem * 10 + v) % 97 } else { (rem * 100 + v) % 97 };
    }
    rem == 1
}

/// Aadhaar numbers: the Verhoeff checksum.
fn verhoeff_ok(s: &str) -> bool {
    const D: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], [1, 2, 3, 4, 0, 6, 7, 8, 9, 5], [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7], [4, 0, 1, 2, 3, 9, 5, 6, 7, 8], [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2], [7, 6, 5, 9, 8, 2, 1, 0, 4, 3], [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const P: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], [1, 5, 7, 6, 2, 8, 3, 0, 9, 4], [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7], [9, 4, 5, 3, 1, 2, 6, 8, 7, 0], [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5], [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    let d = digits(s);
    let c = d.iter().rev().enumerate().fold(0u8, |c, (i, &x)| D[c as usize][P[i % 8][x as usize] as usize]);
    d.len() == 12 && c == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites() {
        assert_eq!(site_from_url("https://mail.google.com/mail/u/0/#inbox?x=1").as_deref(), Some("mail.google.com/mail/u/0"));
        assert_eq!(site_from_url("Example.com/a?token=abc").as_deref(), Some("example.com/a"));
        assert_eq!(site_from_url("chrome://password-manager/passwords").as_deref(), Some("chrome://password-manager/passwords"));
        assert_eq!(site_from_url("how to bake bread"), None); // search words in the address bar
        assert_eq!(site_from_url("newtab"), None);
    }

    #[test]
    fn never_captured() {
        assert!(never_capture("KeePassXC.exe", "db.kdbx", None));
        assert!(never_capture("chrome.exe", "New Tab - Google Chrome (Incognito)", None));
        assert!(never_capture("msedge.exe", "Bing - [InPrivate] - Microsoft Edge", None));
        assert!(never_capture("chrome.exe", "x", Some("netbanking.hdfcbank.com/netbanking")));
        assert!(never_capture("chrome.exe", "x", Some("github.com/login")));
        assert!(never_capture("chrome.exe", "x", Some("chrome://password-manager/passwords")));
        assert!(!never_capture("chrome.exe", "Inbox - Gmail", Some("mail.google.com/mail/u/0")));
        assert!(!never_capture("chrome.exe", "x", Some("blog.example.com/loginless-apps"))); // whole segments only
        assert!(!never_capture("chrome.exe", "x", Some("nothdfcbank.com"))); // not a subdomain
    }

    #[test]
    fn ignore_list() {
        assert_eq!(ignore_entry(" Signal ").as_deref(), Some("signal.exe"));
        assert_eq!(ignore_entry("Discord.exe").as_deref(), Some("discord.exe"));
        assert_eq!(ignore_entry("https://web.whatsapp.com/chat?x=1").as_deref(), Some("web.whatsapp.com"));
        assert_eq!(ignore_entry("   "), None);
        assert!(ignored("signal.exe", "Signal.exe", None));
        assert!(ignored("whatsapp.com", "chrome.exe", Some("web.whatsapp.com/x")));
        assert!(!ignored("whatsapp.com", "chrome.exe", Some("notwhatsapp.com")));
        assert!(!ignored("whatsapp.com", "chrome.exe", None));
    }

    #[test]
    fn masks() {
        assert_eq!(mask("card 4111 1111 1111 1111 ok"), "card [card] ok");
        assert_eq!(mask("order 1234 5678 9012 3456"), "order 1234 5678 9012 3456"); // fails Luhn: just a number
        assert_eq!(mask("pay to GB82 WEST 1234 5698 7654 32 now"), "pay to [iban] now");
        assert_eq!(mask("PAN ABCPE1234F"), "PAN [pan]");
        assert_eq!(mask("key gsk_abcdefghijklmnopqrstuvwxyz0123"), "key [key]");
        assert_eq!(mask("password: hunter2 and more"), "password: [secret] and more");
        assert_eq!(mask("-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----"), "[key]");
        assert_eq!(mask("call 98765 43210"), "call 98765 43210"); // phone numbers stay
        // secrets in links: the value goes, the rest of the link stays readable
        assert_eq!(
            mask("https://medium.com/@me/post-5557b10c0b85?sk=0a1b2c3d4e5f6a7b and more"),
            "https://medium.com/@me/post-5557b10c0b85?sk=[secret] and more"
        );
        assert_eq!(
            mask("accounts.google.com/o/oauth2/auth?client_id=1.apps&code_challenge=Ab_9-x&scope=email"),
            "accounts.google.com/o/oauth2/auth?client_id=1.apps&code_challenge=[secret]&scope=email"
        );
        assert_eq!(mask("google.com/search?q=desk&skin=1"), "google.com/search?q=desk&skin=1");
        // Aadhaar: the one Verhoeff-valid last digit is masked, any other isn't
        let base = "2345 6789 012";
        let ok = (0..10).find(|d| verhoeff_ok(&format!("{base}{d}"))).unwrap();
        assert_eq!(mask(&format!("id {base}{ok}")), "id [aadhaar]");
        assert_eq!(mask(&format!("id {base}{}", (ok + 1) % 10)), format!("id {base}{}", (ok + 1) % 10));
        let once = mask("4111111111111111 password=x");
        assert_eq!(mask(&once), once); // masking masked text changes nothing
    }
}
