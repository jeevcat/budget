use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::Result;
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

    /// Return the earliest expiry time across all cookies, if any have expiry set.
    #[must_use]
    pub fn earliest_expiry(&self) -> Option<DateTime<Utc>> {
        self.cookies
            .iter()
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
    fn earliest_expiry_returns_minimum() {
        let cookies = vec![
            AmazonCookie {
                name: "a".into(),
                value: "1".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(2_000_000_000),
            },
            AmazonCookie {
                name: "b".into(),
                value: "2".into(),
                domain: ".amazon.de".into(),
                path: "/".into(),
                expires: Some(1_500_000_000),
            },
        ];
        let store = CookieStore::from_cookies(cookies, PathBuf::from("/tmp/test.json"));
        let earliest = store.earliest_expiry().unwrap();
        assert_eq!(earliest.timestamp(), 1_500_000_000);
    }
}
