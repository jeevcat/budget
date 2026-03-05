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

impl_sqlx_text!(Iban);

// ---------------------------------------------------------------------------
// Bic — Business Identifier Code (SWIFT code)
// ---------------------------------------------------------------------------

/// A validated BIC / SWIFT code.
///
/// Invariants:
/// - 8 or 11 uppercase alphanumeric characters
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

impl_sqlx_text!(Bic);

// ---------------------------------------------------------------------------
// MerchantCategoryCode — ISO 18245 (4 ASCII digits)
// ---------------------------------------------------------------------------

/// A validated ISO 18245 Merchant Category Code (e.g. "5411" = grocery).
///
/// Invariants:
/// - Exactly 4 ASCII digits
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

impl_sqlx_text!(MerchantCategoryCode);

// ---------------------------------------------------------------------------
// ExchangeRateType — closed enum (AGRD, SALE, SPOT)
// ---------------------------------------------------------------------------

/// FX rate type used in currency exchange transactions.
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
}
