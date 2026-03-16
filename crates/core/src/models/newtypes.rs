//! Validated newtypes for banking domain strings.
//!
//! Each type enforces its invariants at construction time (parse, don't validate).
//! DB decoding skips validation to tolerate legacy data.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Error;

// ---------------------------------------------------------------------------
// Shared sqlx boilerplate — delegates to String (TEXT column)
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlx")]
macro_rules! impl_sqlx_text {
    ($ty:ident) => {
        impl sqlx::Type<sqlx::Postgres> for $ty {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <String as sqlx::Type<sqlx::Postgres>>::type_info()
            }

            fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
                <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
            }
        }

        impl sqlx::Encode<'_, sqlx::Postgres> for $ty {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <String as sqlx::Encode<'_, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $ty {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let s = <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
                Ok(Self(s))
            }
        }
    };
}

// ---------------------------------------------------------------------------
// CurrencyCode — ISO 4217 (3 uppercase ASCII letters)
// ---------------------------------------------------------------------------

/// A validated ISO 4217 currency code (e.g. "EUR", "USD").
///
/// Invariants:
/// - Exactly 3 characters
/// - All uppercase ASCII letters (A–Z)
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CurrencyCode(String);

impl CurrencyCode {
    /// Create a new `CurrencyCode`, validating ISO 4217 format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCurrencyCode`] if the value is not exactly
    /// 3 uppercase ASCII letters.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if s.len() != 3 || !s.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(Error::InvalidCurrencyCode(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for CurrencyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CurrencyCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<&str> for CurrencyCode {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl<'de> Deserialize<'de> for CurrencyCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for CurrencyCode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_text!(CurrencyCode);

// ---------------------------------------------------------------------------
// Iban — International Bank Account Number
// ---------------------------------------------------------------------------

/// A validated IBAN (International Bank Account Number).
///
/// Invariants:
/// - 5–34 characters (2-letter country + 2 check digits + 1–30 BBAN)
/// - First 2 characters are uppercase ASCII letters (country code)
/// - Characters 3–4 are ASCII digits (check digits)
/// - Remaining characters are alphanumeric
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Iban(String);

impl Iban {
    /// Create a new `Iban`, validating format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidIban`] if the format is invalid.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if s.len() < 5 || s.len() > 34 {
            return Err(Error::InvalidIban(s));
        }
        let bytes = s.as_bytes();
        if !bytes[0].is_ascii_uppercase() || !bytes[1].is_ascii_uppercase() {
            return Err(Error::InvalidIban(s));
        }
        if !bytes[2].is_ascii_digit() || !bytes[3].is_ascii_digit() {
            return Err(Error::InvalidIban(s));
        }
        if !bytes[4..].iter().all(u8::is_ascii_alphanumeric) {
            return Err(Error::InvalidIban(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for Iban {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Iban {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Iban {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for Iban {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_text!(Iban);

// ---------------------------------------------------------------------------
// Bic — Business Identifier Code (SWIFT code)
// ---------------------------------------------------------------------------

/// A validated BIC / SWIFT code.
///
/// Invariants:
/// - 8 or 11 uppercase alphanumeric characters
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Bic(String);

impl Bic {
    /// Create a new `Bic`, validating format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidBic`] if the format is invalid.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if (s.len() != 8 && s.len() != 11)
            || !s
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(Error::InvalidBic(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for Bic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Bic {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Bic {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for Bic {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_text!(Bic);

// ---------------------------------------------------------------------------
// MerchantCategoryCode — ISO 18245 (4 ASCII digits)
// ---------------------------------------------------------------------------

/// A validated ISO 18245 Merchant Category Code (e.g. "5411" = grocery).
///
/// Invariants:
/// - Exactly 4 ASCII digits
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct MerchantCategoryCode(String);

impl MerchantCategoryCode {
    /// Create a new `MerchantCategoryCode`, validating format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidMerchantCategoryCode`] if the value is not
    /// exactly 4 ASCII digits.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::InvalidMerchantCategoryCode(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for MerchantCategoryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for MerchantCategoryCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for MerchantCategoryCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for MerchantCategoryCode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_text!(MerchantCategoryCode);

// ---------------------------------------------------------------------------
// DomainCode — ISO 20022 domain code
// ---------------------------------------------------------------------------

/// A validated ISO 20022 domain code (e.g. "PMNT", "CAMT", "SECU").
///
/// Invariants:
/// - Non-empty
/// - Uppercase ASCII alphanumeric only (A–Z, 0–9)
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DomainCode(String);

impl DomainCode {
    /// Create a new `DomainCode`, validating format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDomainCode`] if the value is empty or contains
    /// characters other than uppercase ASCII letters and digits.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if s.is_empty()
            || !s
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(Error::InvalidDomainCode(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for DomainCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for DomainCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for DomainCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for DomainCode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_text!(DomainCode);

// ---------------------------------------------------------------------------
// SubFamilyCode — ISO 20022 family-subfamily code
// ---------------------------------------------------------------------------

/// A validated ISO 20022 family-subfamily code (e.g. "ICDT-STDO", "RCDT-ESCT").
///
/// Invariants:
/// - Non-empty
/// - Uppercase ASCII alphanumeric and hyphens only (A–Z, 0–9, `-`)
/// - No leading or trailing hyphens
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = String))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct SubFamilyCode(String);

impl SubFamilyCode {
    /// Create a new `SubFamilyCode`, validating format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidSubFamilyCode`] if the value is empty, contains
    /// invalid characters, or has leading/trailing hyphens.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if s.is_empty()
            || s.starts_with('-')
            || s.ends_with('-')
            || !s
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::InvalidSubFamilyCode(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for SubFamilyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SubFamilyCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SubFamilyCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for SubFamilyCode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_text!(SubFamilyCode);

// ---------------------------------------------------------------------------
// ExchangeRateType — closed enum (AGRD, SALE, SPOT)
// ---------------------------------------------------------------------------

/// FX rate type used in currency exchange transactions.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExchangeRateType {
    /// Agreed/contract rate
    #[serde(rename = "AGRD")]
    Agreed,
    /// Sale rate
    #[serde(rename = "SALE")]
    Sale,
    /// Spot rate
    #[serde(rename = "SPOT")]
    Spot,
}

impl fmt::Display for ExchangeRateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agreed => write!(f, "AGRD"),
            Self::Sale => write!(f, "SALE"),
            Self::Spot => write!(f, "SPOT"),
        }
    }
}

impl std::str::FromStr for ExchangeRateType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AGRD" => Ok(Self::Agreed),
            "SALE" => Ok(Self::Sale),
            "SPOT" => Ok(Self::Spot),
            _ => Err(Error::InvalidExchangeRateType(s.to_owned())),
        }
    }
}

// ---------------------------------------------------------------------------
// ReferenceNumberSchema — semi-open enum (known variants + Other fallback)
// ---------------------------------------------------------------------------

/// Scheme of a structured payment reference number.
///
/// The known variants come from Enable Banking. Since the set may expand,
/// unrecognised values are captured in `Other` rather than rejected.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReferenceNumberSchema {
    /// Belgian structured reference
    #[serde(rename = "BERF")]
    Berf,
    /// Finnish structured reference
    #[serde(rename = "FIRF")]
    Firf,
    /// ISO 11649 international reference
    #[serde(rename = "INTL")]
    Intl,
    /// Norwegian structured reference
    #[serde(rename = "NORF")]
    Norf,
    /// SEPA Direct Debit Mandate reference
    #[serde(rename = "SDDM")]
    Sddm,
    /// Swedish Bankgiro/Plusgiro reference
    #[serde(rename = "SEBG")]
    Sebg,
    /// Unrecognised schema from the provider
    #[serde(untagged)]
    Other(String),
}

impl fmt::Display for ReferenceNumberSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Berf => write!(f, "BERF"),
            Self::Firf => write!(f, "FIRF"),
            Self::Intl => write!(f, "INTL"),
            Self::Norf => write!(f, "NORF"),
            Self::Sddm => write!(f, "SDDM"),
            Self::Sebg => write!(f, "SEBG"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::str::FromStr for ReferenceNumberSchema {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "BERF" => Self::Berf,
            "FIRF" => Self::Firf,
            "INTL" => Self::Intl,
            "NORF" => Self::Norf,
            "SDDM" => Self::Sddm,
            "SEBG" => Self::Sebg,
            other => Self::Other(other.to_owned()),
        })
    }
}

// ---------------------------------------------------------------------------
// Shared sqlx boilerplate — delegates to an integer column
// ---------------------------------------------------------------------------

#[cfg(feature = "sqlx")]
macro_rules! impl_sqlx_int {
    ($ty:ident, $inner:ty) => {
        impl sqlx::Type<sqlx::Postgres> for $ty {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <$inner as sqlx::Type<sqlx::Postgres>>::type_info()
            }

            fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
                <$inner as sqlx::Type<sqlx::Postgres>>::compatible(ty)
            }
        }

        impl sqlx::Encode<'_, sqlx::Postgres> for $ty {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <$inner as sqlx::Encode<'_, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $ty {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                let v = <$inner as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
                Ok(Self(v))
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Priority — rule ordering (0–1000)
// ---------------------------------------------------------------------------

/// A validated rule priority (0–1000).
///
/// Higher values indicate higher priority. Rules are evaluated in descending
/// priority order; the first match wins.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = i32))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Priority(i32);

impl Priority {
    /// Create a new `Priority`, validating the range 0–1000.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidPriority`] if the value is outside 0–1000.
    pub fn new(value: i32) -> Result<Self, Error> {
        if !(0..=1000).contains(&value) {
            return Err(Error::InvalidPriority(value));
        }
        Ok(Self(value))
    }

    /// Return the inner `i32` value.
    #[must_use]
    pub fn get(self) -> i32 {
        self.0
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for Priority {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = i32::deserialize(deserializer)?;
        Self::new(v).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "sqlx")]
impl_sqlx_int!(Priority, i32);

// ---------------------------------------------------------------------------
// ValidDays — authorization validity window (1–365)
// ---------------------------------------------------------------------------

/// A validated authorization validity period in days (1–365).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(value_type = u32))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ValidDays(u32);

impl ValidDays {
    /// Create a new `ValidDays`, validating the range 1–365.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidValidDays`] if the value is outside 1–365.
    pub fn new(value: u32) -> Result<Self, Error> {
        if !(1..=365).contains(&value) {
            return Err(Error::InvalidValidDays(value));
        }
        Ok(Self(value))
    }

    /// Return the inner `u32` value.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ValidDays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for ValidDays {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(deserializer)?;
        Self::new(v).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// DatabaseUrl — PostgreSQL connection string
// ---------------------------------------------------------------------------

/// A validated `PostgreSQL` connection URL.
///
/// Invariants:
/// - Non-empty
/// - Starts with `postgresql://` or `postgres://`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    /// Create a new `DatabaseUrl`, validating the scheme prefix.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDatabaseUrl`] if the value does not start with
    /// `postgresql://` or `postgres://`.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if !s.starts_with("postgresql://") && !s.starts_with("postgres://") {
            return Err(Error::InvalidDatabaseUrl(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for DatabaseUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for DatabaseUrl {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for DatabaseUrl {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// SecretKey — API authentication token
// ---------------------------------------------------------------------------

/// A validated API secret key.
///
/// Invariants:
/// - Empty (development/unconfigured) OR at least 8 characters
#[derive(Debug, Clone, Eq, Serialize)]
#[serde(transparent)]
pub struct SecretKey(String);

impl PartialEq for SecretKey {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq as _;
        self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
    }
}

impl std::hash::Hash for SecretKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl SecretKey {
    /// Create a new `SecretKey`, validating minimum length.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidSecretKey`] if the value is non-empty but
    /// shorter than 8 characters.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if !s.is_empty() && s.len() < 8 {
            return Err(Error::InvalidSecretKey);
        }
        Ok(Self(s))
    }

    /// An empty secret key (unconfigured / development mode).
    #[must_use]
    pub fn empty() -> Self {
        Self(String::new())
    }
}

impl fmt::Display for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SecretKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SecretKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for SecretKey {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// Host — public base URL
// ---------------------------------------------------------------------------

/// A validated public base URL (e.g. `https://budget.example.com`).
///
/// Invariants:
/// - Starts with `http://` or `https://`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Host(String);

impl Host {
    /// Create a new `Host`, validating the scheme prefix.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidHost`] if the value does not start with
    /// `http://` or `https://`.
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        if !s.starts_with("http://") && !s.starts_with("https://") {
            return Err(Error::InvalidHost(s));
        }
        Ok(Self(s))
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Host {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<Host> for String {
    fn from(h: Host) -> Self {
        h.0
    }
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for Host {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- CurrencyCode -------------------------------------------------------

    #[test]
    fn currency_code_valid() {
        assert!(CurrencyCode::new("EUR").is_ok());
        assert!(CurrencyCode::new("USD").is_ok());
        assert!(CurrencyCode::new("GBP").is_ok());
    }

    #[test]
    fn currency_code_rejects_lowercase() {
        assert!(CurrencyCode::new("eur").is_err());
    }

    #[test]
    fn currency_code_rejects_wrong_length() {
        assert!(CurrencyCode::new("EU").is_err());
        assert!(CurrencyCode::new("EURO").is_err());
    }

    #[test]
    fn currency_code_rejects_digits() {
        assert!(CurrencyCode::new("US1").is_err());
    }

    #[test]
    fn currency_code_rejects_empty() {
        assert!(CurrencyCode::new("").is_err());
    }

    #[test]
    fn currency_code_display() {
        let c = CurrencyCode::new("SEK").unwrap();
        assert_eq!(c.to_string(), "SEK");
    }

    #[test]
    fn currency_code_serde_roundtrip() {
        let c = CurrencyCode::new("EUR").unwrap();
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"EUR\"");
        let back: CurrencyCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn currency_code_serde_rejects_invalid() {
        let result: Result<CurrencyCode, _> = serde_json::from_str("\"eu\"");
        assert!(result.is_err());
    }

    // -- Iban ---------------------------------------------------------------

    #[test]
    fn iban_valid() {
        assert!(Iban::new("DE89370400440532013000").is_ok());
        assert!(Iban::new("GB29NWBK60161331926819").is_ok());
        assert!(Iban::new("FI2112345600000785").is_ok());
    }

    #[test]
    fn iban_rejects_too_short() {
        assert!(Iban::new("DE89").is_err());
    }

    #[test]
    fn iban_rejects_too_long() {
        let long = format!("DE89{}", "0".repeat(31));
        assert!(Iban::new(long).is_err());
    }

    #[test]
    fn iban_rejects_lowercase_country() {
        assert!(Iban::new("de89370400440532013000").is_err());
    }

    #[test]
    fn iban_rejects_non_digit_check() {
        assert!(Iban::new("DEAB370400440532013000").is_err());
    }

    #[test]
    fn iban_display() {
        let iban = Iban::new("FI2112345600000785").unwrap();
        assert_eq!(iban.to_string(), "FI2112345600000785");
    }

    #[test]
    fn iban_serde_roundtrip() {
        let iban = Iban::new("DE89370400440532013000").unwrap();
        let json = serde_json::to_string(&iban).unwrap();
        assert_eq!(json, "\"DE89370400440532013000\"");
        let back: Iban = serde_json::from_str(&json).unwrap();
        assert_eq!(back, iban);
    }

    // -- Bic ----------------------------------------------------------------

    #[test]
    fn bic_valid_8() {
        assert!(Bic::new("DEUTDEFF").is_ok());
    }

    #[test]
    fn bic_valid_11() {
        assert!(Bic::new("DEUTDEFF500").is_ok());
    }

    #[test]
    fn bic_rejects_wrong_length() {
        assert!(Bic::new("DEUTDEF").is_err());
        assert!(Bic::new("DEUTDEFF5").is_err());
        assert!(Bic::new("DEUTDEFF50").is_err());
        assert!(Bic::new("DEUTDEFF5001").is_err());
    }

    #[test]
    fn bic_rejects_lowercase() {
        assert!(Bic::new("deutdeff").is_err());
    }

    #[test]
    fn bic_display() {
        let bic = Bic::new("DEUTDEFF").unwrap();
        assert_eq!(bic.to_string(), "DEUTDEFF");
    }

    #[test]
    fn bic_serde_roundtrip() {
        let bic = Bic::new("DEUTDEFF500").unwrap();
        let json = serde_json::to_string(&bic).unwrap();
        assert_eq!(json, "\"DEUTDEFF500\"");
        let back: Bic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, bic);
    }

    // -- MerchantCategoryCode -----------------------------------------------

    #[test]
    fn mcc_valid() {
        assert!(MerchantCategoryCode::new("5411").is_ok());
        assert!(MerchantCategoryCode::new("0000").is_ok());
        assert!(MerchantCategoryCode::new("9999").is_ok());
    }

    #[test]
    fn mcc_rejects_wrong_length() {
        assert!(MerchantCategoryCode::new("541").is_err());
        assert!(MerchantCategoryCode::new("54111").is_err());
    }

    #[test]
    fn mcc_rejects_non_digits() {
        assert!(MerchantCategoryCode::new("541A").is_err());
    }

    #[test]
    fn mcc_display() {
        let mcc = MerchantCategoryCode::new("5411").unwrap();
        assert_eq!(mcc.to_string(), "5411");
    }

    #[test]
    fn mcc_serde_roundtrip() {
        let mcc = MerchantCategoryCode::new("5411").unwrap();
        let json = serde_json::to_string(&mcc).unwrap();
        assert_eq!(json, "\"5411\"");
        let back: MerchantCategoryCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mcc);
    }

    // -- DomainCode ---------------------------------------------------------

    #[test]
    fn domain_code_valid() {
        assert!(DomainCode::new("PMNT").is_ok());
        assert!(DomainCode::new("CAMT").is_ok());
        assert!(DomainCode::new("SECU").is_ok());
        assert!(DomainCode::new("X1").is_ok());
    }

    #[test]
    fn domain_code_rejects_empty() {
        assert!(DomainCode::new("").is_err());
    }

    #[test]
    fn domain_code_rejects_lowercase() {
        assert!(DomainCode::new("pmnt").is_err());
        assert!(DomainCode::new("Pmnt").is_err());
    }

    #[test]
    fn domain_code_rejects_special_chars() {
        assert!(DomainCode::new("PM-NT").is_err());
        assert!(DomainCode::new("PM NT").is_err());
        assert!(DomainCode::new("PM_NT").is_err());
    }

    #[test]
    fn domain_code_display() {
        let dc = DomainCode::new("PMNT").unwrap();
        assert_eq!(dc.to_string(), "PMNT");
    }

    #[test]
    fn domain_code_serde_roundtrip() {
        let dc = DomainCode::new("PMNT").unwrap();
        let json = serde_json::to_string(&dc).unwrap();
        assert_eq!(json, "\"PMNT\"");
        let back: DomainCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, dc);
    }

    #[test]
    fn domain_code_serde_rejects_invalid() {
        let result: Result<DomainCode, _> = serde_json::from_str("\"pm\"");
        assert!(result.is_err());
    }

    // -- SubFamilyCode ------------------------------------------------------

    #[test]
    fn sub_family_code_valid() {
        assert!(SubFamilyCode::new("ICDT-STDO").is_ok());
        assert!(SubFamilyCode::new("RCDT-ESCT").is_ok());
        assert!(SubFamilyCode::new("STDO").is_ok());
        assert!(SubFamilyCode::new("A1-B2-C3").is_ok());
    }

    #[test]
    fn sub_family_code_rejects_empty() {
        assert!(SubFamilyCode::new("").is_err());
    }

    #[test]
    fn sub_family_code_rejects_lowercase() {
        assert!(SubFamilyCode::new("icdt-stdo").is_err());
    }

    #[test]
    fn sub_family_code_rejects_leading_hyphen() {
        assert!(SubFamilyCode::new("-STDO").is_err());
    }

    #[test]
    fn sub_family_code_rejects_trailing_hyphen() {
        assert!(SubFamilyCode::new("STDO-").is_err());
    }

    #[test]
    fn sub_family_code_rejects_special_chars() {
        assert!(SubFamilyCode::new("ICDT STDO").is_err());
        assert!(SubFamilyCode::new("ICDT_STDO").is_err());
    }

    #[test]
    fn sub_family_code_display() {
        let sc = SubFamilyCode::new("ICDT-STDO").unwrap();
        assert_eq!(sc.to_string(), "ICDT-STDO");
    }

    #[test]
    fn sub_family_code_serde_roundtrip() {
        let sc = SubFamilyCode::new("ICDT-STDO").unwrap();
        let json = serde_json::to_string(&sc).unwrap();
        assert_eq!(json, "\"ICDT-STDO\"");
        let back: SubFamilyCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sc);
    }

    #[test]
    fn sub_family_code_serde_rejects_invalid() {
        let result: Result<SubFamilyCode, _> = serde_json::from_str("\"-BAD\"");
        assert!(result.is_err());
    }

    // -- ExchangeRateType ---------------------------------------------------

    #[test]
    fn exchange_rate_type_display_roundtrip() {
        for (variant, expected) in [
            (ExchangeRateType::Agreed, "AGRD"),
            (ExchangeRateType::Sale, "SALE"),
            (ExchangeRateType::Spot, "SPOT"),
        ] {
            assert_eq!(variant.to_string(), expected);
            assert_eq!(expected.parse::<ExchangeRateType>().unwrap(), variant);
        }
    }

    #[test]
    fn exchange_rate_type_from_str_invalid() {
        assert!("UNKNOWN".parse::<ExchangeRateType>().is_err());
    }

    #[test]
    fn exchange_rate_type_serde_roundtrip() {
        for (variant, expected_json) in [
            (ExchangeRateType::Agreed, "\"AGRD\""),
            (ExchangeRateType::Sale, "\"SALE\""),
            (ExchangeRateType::Spot, "\"SPOT\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json);
            let back: ExchangeRateType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    // -- ReferenceNumberSchema ----------------------------------------------

    #[test]
    fn ref_schema_known_variants() {
        for (s, expected) in [
            ("BERF", ReferenceNumberSchema::Berf),
            ("FIRF", ReferenceNumberSchema::Firf),
            ("INTL", ReferenceNumberSchema::Intl),
            ("NORF", ReferenceNumberSchema::Norf),
            ("SDDM", ReferenceNumberSchema::Sddm),
            ("SEBG", ReferenceNumberSchema::Sebg),
        ] {
            assert_eq!(s.parse::<ReferenceNumberSchema>().unwrap(), expected);
            assert_eq!(expected.to_string(), s);
        }
    }

    #[test]
    fn ref_schema_unknown_falls_back_to_other() {
        let schema: ReferenceNumberSchema = "NEWX".parse().unwrap();
        assert_eq!(schema, ReferenceNumberSchema::Other("NEWX".to_owned()));
        assert_eq!(schema.to_string(), "NEWX");
    }

    #[test]
    fn ref_schema_serde_known() {
        let json = serde_json::to_string(&ReferenceNumberSchema::Firf).unwrap();
        assert_eq!(json, "\"FIRF\"");
        let back: ReferenceNumberSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ReferenceNumberSchema::Firf);
    }

    #[test]
    fn ref_schema_serde_unknown() {
        let back: ReferenceNumberSchema = serde_json::from_str("\"NEWX\"").unwrap();
        assert_eq!(back, ReferenceNumberSchema::Other("NEWX".to_owned()));
    }

    // -- Priority -----------------------------------------------------------

    #[test]
    fn priority_valid_boundaries() {
        assert!(Priority::new(0).is_ok());
        assert!(Priority::new(1000).is_ok());
        assert!(Priority::new(500).is_ok());
    }

    #[test]
    fn priority_rejects_out_of_range() {
        assert!(Priority::new(-1).is_err());
        assert!(Priority::new(1001).is_err());
        assert!(Priority::new(i32::MIN).is_err());
        assert!(Priority::new(i32::MAX).is_err());
    }

    #[test]
    fn priority_default_is_zero() {
        assert_eq!(Priority::default().get(), 0);
    }

    #[test]
    fn priority_display() {
        assert_eq!(Priority::new(42).unwrap().to_string(), "42");
    }

    #[test]
    fn priority_serde_roundtrip() {
        let p = Priority::new(10).unwrap();
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "10");
        let back: Priority = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn priority_serde_rejects_invalid() {
        assert!(serde_json::from_str::<Priority>("1001").is_err());
        assert!(serde_json::from_str::<Priority>("-1").is_err());
    }

    // -- ValidDays ----------------------------------------------------------

    #[test]
    fn valid_days_boundaries() {
        assert!(ValidDays::new(1).is_ok());
        assert!(ValidDays::new(365).is_ok());
        assert!(ValidDays::new(90).is_ok());
    }

    #[test]
    fn valid_days_rejects_out_of_range() {
        assert!(ValidDays::new(0).is_err());
        assert!(ValidDays::new(366).is_err());
        assert!(ValidDays::new(u32::MAX).is_err());
    }

    #[test]
    fn valid_days_display() {
        assert_eq!(ValidDays::new(90).unwrap().to_string(), "90");
    }

    #[test]
    fn valid_days_serde_roundtrip() {
        let v = ValidDays::new(90).unwrap();
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "90");
        let back: ValidDays = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn valid_days_serde_rejects_invalid() {
        assert!(serde_json::from_str::<ValidDays>("0").is_err());
        assert!(serde_json::from_str::<ValidDays>("366").is_err());
    }

    // -- DatabaseUrl --------------------------------------------------------

    #[test]
    fn database_url_valid() {
        assert!(DatabaseUrl::new("postgresql://user@localhost/db").is_ok());
        assert!(DatabaseUrl::new("postgres://user@localhost/db").is_ok());
    }

    #[test]
    fn database_url_rejects_invalid_scheme() {
        assert!(DatabaseUrl::new("mysql://user@localhost/db").is_err());
        assert!(DatabaseUrl::new("http://localhost").is_err());
        assert!(DatabaseUrl::new("").is_err());
    }

    #[test]
    fn database_url_display() {
        let url = DatabaseUrl::new("postgresql://budget@localhost:5432/budget").unwrap();
        assert_eq!(url.to_string(), "postgresql://budget@localhost:5432/budget");
    }

    #[test]
    fn database_url_serde_roundtrip() {
        let url = DatabaseUrl::new("postgresql://budget@localhost/db").unwrap();
        let json = serde_json::to_string(&url).unwrap();
        assert_eq!(json, "\"postgresql://budget@localhost/db\"");
        let back: DatabaseUrl = serde_json::from_str(&json).unwrap();
        assert_eq!(back, url);
    }

    #[test]
    fn database_url_serde_rejects_invalid() {
        let result: Result<DatabaseUrl, _> = serde_json::from_str("\"mysql://x\"");
        assert!(result.is_err());
    }

    // -- SecretKey ----------------------------------------------------------

    #[test]
    fn secret_key_empty_allowed() {
        assert!(SecretKey::new("").is_ok());
        assert_eq!(SecretKey::empty().as_ref(), "");
    }

    #[test]
    fn secret_key_valid_long() {
        assert!(SecretKey::new("abcdefgh").is_ok());
        assert!(SecretKey::new("a]very-long-secret-key-1234").is_ok());
    }

    #[test]
    fn secret_key_rejects_short() {
        assert!(SecretKey::new("abc").is_err());
        assert!(SecretKey::new("1234567").is_err());
    }

    #[test]
    fn secret_key_equality() {
        let a = SecretKey::new("my-secret-key").unwrap();
        let b = SecretKey::new("my-secret-key").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn secret_key_serde_roundtrip() {
        let sk = SecretKey::new("my-secret-key").unwrap();
        let json = serde_json::to_string(&sk).unwrap();
        assert_eq!(json, "\"my-secret-key\"");
        let back: SecretKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sk);
    }

    #[test]
    fn secret_key_serde_rejects_short() {
        let result: Result<SecretKey, _> = serde_json::from_str("\"abc\"");
        assert!(result.is_err());
    }

    // -- Host ---------------------------------------------------------------

    #[test]
    fn host_valid() {
        assert!(Host::new("http://localhost:3000").is_ok());
        assert!(Host::new("https://budget.example.com").is_ok());
    }

    #[test]
    fn host_rejects_invalid_scheme() {
        assert!(Host::new("ftp://example.com").is_err());
        assert!(Host::new("localhost:3000").is_err());
        assert!(Host::new("").is_err());
    }

    #[test]
    fn host_display() {
        let h = Host::new("https://budget.example.com").unwrap();
        assert_eq!(h.to_string(), "https://budget.example.com");
    }

    #[test]
    fn host_serde_roundtrip() {
        let h = Host::new("https://budget.example.com").unwrap();
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, "\"https://budget.example.com\"");
        let back: Host = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn host_serde_rejects_invalid() {
        let result: Result<Host, _> = serde_json::from_str("\"ftp://x\"");
        assert!(result.is_err());
    }
}
