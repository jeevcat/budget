use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{AmazonError, Result};
use crate::types::AmazonCookie;

/// Manages Amazon session cookies loaded from a JSON file.
pub struct CookieStore {
    cookies: Vec<AmazonCookie>,
    path: PathBuf,
}

impl CookieStore {
    /// Load cookies from a JSON file at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed as JSON.
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)?;
        let cookies: Vec<AmazonCookie> = serde_json::from_str(&data)?;
        Ok(Self {
            cookies,
            path: path.to_owned(),
        })
    }

    /// Create a `CookieStore` from an already-parsed cookie list and a path for saving.
    #[must_use]
    pub fn from_cookies(cookies: Vec<AmazonCookie>, path: PathBuf) -> Self {
        Self { cookies, path }
    }

    /// Save the current cookies back to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or file writing fails.
    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_string_pretty(&self.cookies)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }

    /// Format all cookies as a `Cookie` header value.
    #[must_use]
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Check if the auth token cookie has expired.
    ///
    /// Looks for cookies matching `at-*` (auth token). If any auth token
    /// has an expiry in the past, returns `true`.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        let now = Utc::now().timestamp();
        let auth_cookies: Vec<_> = self
            .cookies
            .iter()
            .filter(|c| c.name.starts_with("at-") || c.name.starts_with("sess-at-"))
            .collect();

        if auth_cookies.is_empty() {
            return true;
        }

        auth_cookies
            .iter()
            .any(|c| c.expires.is_some_and(|exp| exp < now))
    }

    /// Return the earliest auth token expiry time.
    ///
    /// Only considers `at-*` and `sess-at-*` cookies (the actual auth tokens),
    /// not session cookies that may expire sooner but are less important.
    #[must_use]
    pub fn earliest_expiry(&self) -> Option<DateTime<Utc>> {
        self.cookies
            .iter()
            .filter(|c| c.name.starts_with("at-") || c.name.starts_with("sess-at-"))
            .filter_map(|c| c.expires)
            .min()
            .and_then(|ts| DateTime::from_timestamp(ts, 0))
    }

    /// Return a reference to the cookie path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return a reference to the underlying cookies.
    #[must_use]
    pub fn cookies(&self) -> &[AmazonCookie] {
        &self.cookies
    }

    /// Parse cookies from text, auto-detecting format (JSON array or Netscape cookies.txt).
    ///
    /// # Errors
    ///
    /// Returns an error if the text cannot be parsed as either format.
    pub fn parse_cookies_auto(text: &str) -> Result<Vec<AmazonCookie>> {
        let trimmed = text.trim();

        // Try JSON first — starts with '[' or '{' (some exports wrap in an object)
        if trimmed.starts_with('[') {
            let cookies: Vec<AmazonCookie> = serde_json::from_str(trimmed)?;
            return Ok(cookies);
        }

        // Otherwise try Netscape cookies.txt format
        parse_netscape_cookies(trimmed)
    }
}

/// Parse Netscape/Mozilla cookies.txt format.
///
/// Format: tab-separated fields per line:
/// `domain \t include_subdomains \t path \t secure \t expires \t name \t value`
///
/// Lines starting with `#` or that are blank are skipped.
fn parse_netscape_cookies(text: &str) -> Result<Vec<AmazonCookie>> {
    let mut cookies = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            continue;
        }

        let expires = fields[4].parse::<i64>().ok().filter(|&e| e > 0);

        cookies.push(AmazonCookie {
            domain: fields[0].to_owned(),
            path: fields[2].to_owned(),
            expires,
            name: fields[5].to_owned(),
            value: fields[6].to_owned(),
        });
    }

    if cookies.is_empty() {
        return Err(AmazonError::Parse(
            "no cookies found — expected JSON array or Netscape cookies.txt format".to_owned(),
        ));
    }

    Ok(cookies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_cookies(expires: i64) -> Vec<AmazonCookie> {
        vec![
            AmazonCookie {
                name: "session-id".into(),
                value: "123-456-789".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(expires),
            },
            AmazonCookie {
                name: "at-acbde".into(),
                value: "auth-token-value".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(expires),
            },
            AmazonCookie {
                name: "x-acbde".into(),
                value: "account-id".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(expires),
            },
        ]
    }

    #[test]
    fn load_save_roundtrip() {
        let future = Utc::now().timestamp() + 86400;
        let cookies = sample_cookies(future);

        let mut tmpfile = NamedTempFile::new().unwrap();
        write!(tmpfile, "{}", serde_json::to_string(&cookies).unwrap()).unwrap();

        let store = CookieStore::load(tmpfile.path()).unwrap();
        assert_eq!(store.cookies().len(), 3);

        // Save and reload
        store.save().unwrap();
        let reloaded = CookieStore::load(tmpfile.path()).unwrap();
        assert_eq!(reloaded.cookies().len(), 3);
        assert_eq!(reloaded.cookies()[0].name, "session-id");
    }

    #[test]
    fn is_expired_with_future_expiry() {
        let future = Utc::now().timestamp() + 86400;
        let store =
            CookieStore::from_cookies(sample_cookies(future), PathBuf::from("/tmp/test.json"));
        assert!(!store.is_expired());
    }

    #[test]
    fn is_expired_with_past_expiry() {
        let past = Utc::now().timestamp() - 86400;
        let store =
            CookieStore::from_cookies(sample_cookies(past), PathBuf::from("/tmp/test.json"));
        assert!(store.is_expired());
    }

    #[test]
    fn is_expired_when_no_auth_cookie() {
        let cookies = vec![AmazonCookie {
            name: "session-id".into(),
            value: "123".into(),
            domain: ".amazon.de".into(),
            path: "/".into(),
            expires: Some(Utc::now().timestamp() + 86400),
        }];
        let store = CookieStore::from_cookies(cookies, PathBuf::from("/tmp/test.json"));
        assert!(store.is_expired());
    }

    #[test]
    fn cookie_header_formatting() {
        let future = Utc::now().timestamp() + 86400;
        let store =
            CookieStore::from_cookies(sample_cookies(future), PathBuf::from("/tmp/test.json"));
        let header = store.cookie_header();
        assert!(header.contains("session-id=123-456-789"));
        assert!(header.contains("at-acbde=auth-token-value"));
        assert!(header.contains("; "));
    }

    #[test]
    fn parse_netscape_format() {
        let text = "# Netscape HTTP Cookie File\n\
                     .amazon.de\tTRUE\t/\tFALSE\t2000000000\tsession-id\t123-456-789\n\
                     .amazon.de\tTRUE\t/\tTRUE\t2000000000\tat-acbde\tauth-token-value\n\
                     # a comment\n\
                     \n\
                     .amazon.de\tTRUE\t/\tFALSE\t0\tx-acbde\taccount-id\n";

        let cookies = CookieStore::parse_cookies_auto(text).unwrap();
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0].name, "session-id");
        assert_eq!(cookies[0].value, "123-456-789");
        assert_eq!(cookies[0].domain, ".amazon.de");
        assert_eq!(cookies[0].expires, Some(2_000_000_000));
        assert_eq!(cookies[1].name, "at-acbde");
        // expires=0 is treated as no expiry
        assert_eq!(cookies[2].expires, None);
    }

    #[test]
    fn parse_auto_detects_json() {
        let json = r#"[{"name":"at-x","value":"v","domain":".amazon.de","path":"/","expires":2000000000}]"#;
        let cookies = CookieStore::parse_cookies_auto(json).unwrap();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "at-x");
    }

    #[test]
    fn parse_empty_text_errors() {
        assert!(CookieStore::parse_cookies_auto("").is_err());
    }

    #[test]
    fn parse_comments_only_errors() {
        let text = "# Netscape HTTP Cookie File\n# nothing else\n";
        assert!(CookieStore::parse_cookies_auto(text).is_err());
    }

    #[test]
    fn earliest_expiry_returns_minimum_auth_token() {
        let cookies = vec![
            AmazonCookie {
                name: "at-acbde".into(),
                value: "1".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(2_000_000_000),
            },
            AmazonCookie {
                name: "sess-at-acbde".into(),
                value: "2".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(1_500_000_000),
            },
            AmazonCookie {
                name: "session-id".into(),
                value: "3".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(1_000_000_000),
            },
        ];
        let store = CookieStore::from_cookies(cookies, PathBuf::from("/tmp/test.json"));
        let earliest = store.earliest_expiry().unwrap();
        // Should pick sess-at-acbde (1_500_000_000), not session-id (1_000_000_000)
        assert_eq!(earliest.timestamp(), 1_500_000_000);
    }
}
