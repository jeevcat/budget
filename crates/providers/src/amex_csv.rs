//! Amex CSV import parser.
//!
//! Parses CSV exports from the American Express EU web portal (German locale)
//! into [`Transaction`] structs that feed into the existing upsert pipeline.
//! Deduplication relies on the `Betreff` column as `provider_transaction_id`.

use chrono::NaiveDate;
use regex::Regex;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::LazyLock;

use crate::bank::Transaction;

/// Errors that can occur when parsing an Amex CSV export.
#[derive(Debug, thiserror::Error)]
pub enum AmexCsvError {
    #[error("CSV parsing error: {0}")]
    Csv(#[from] csv::Error),

    #[error("row {row}: invalid date \"{value}\"")]
    InvalidDate { row: usize, value: String },

    #[error("row {row}: invalid amount \"{value}\"")]
    InvalidAmount { row: usize, value: String },

    #[error("row {row}: missing Betreff (provider transaction ID)")]
    MissingBetreff { row: usize },

    #[error("CSV file is empty (no data rows)")]
    Empty,
}

/// A single row from the Amex CSV, mapped via serde from German headers.
#[derive(Debug, serde::Deserialize)]
struct AmexRow {
    #[serde(rename = "Datum")]
    datum: String,
    #[serde(rename = "Beschreibung")]
    beschreibung: String,
    #[serde(rename = "Karteninhaber")]
    karteninhaber: String,
    #[serde(rename = "Konto #")]
    _konto: String,
    #[serde(rename = "Betrag")]
    betrag: String,
    #[serde(rename = "Weitere Details")]
    weitere_details: String,
    #[serde(rename = "Erscheint auf Ihrer Abrechnung als")]
    _erscheint_als: String,
    #[serde(rename = "Adresse")]
    _adresse: String,
    #[serde(rename = "Stadt")]
    _stadt: String,
    #[serde(rename = "PLZ")]
    _plz: String,
    #[serde(rename = "Land")]
    _land: String,
    #[serde(rename = "Betreff")]
    betreff: String,
    #[serde(rename = "Kategorie")]
    kategorie: String,
}

/// Parsed foreign-currency details from the `Weitere Details` field.
struct FxInfo {
    foreign_amount: Decimal,
    currency: String,
    exchange_rate: String,
}

static FX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"Foreign Spend Amount:\s*([\d,]+(?:\.\d+)?)\s+(.+?)\s+Commission Amount:.*?Currency Exchange Rate:\s*([\d.]+)",
    ).expect("valid regex")
});

/// Parse an Amex CSV export (German locale) into provider transactions.
///
/// # Errors
///
/// Returns [`AmexCsvError`] if the CSV is malformed, empty, or contains
/// unparseable dates/amounts.
pub fn parse_amex_csv(input: &str) -> Result<Vec<Transaction>, AmexCsvError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(input.as_bytes());

    let mut transactions = Vec::new();

    for (i, result) in reader.deserialize().enumerate() {
        let row_num = i + 2; // 1-indexed, header is row 1
        let row: AmexRow = result?;
        transactions.push(row_to_transaction(&row, row_num)?);
    }

    if transactions.is_empty() {
        return Err(AmexCsvError::Empty);
    }

    Ok(transactions)
}

fn row_to_transaction(row: &AmexRow, row_num: usize) -> Result<Transaction, AmexCsvError> {
    let betreff = row.betreff.trim().trim_matches('\'').trim();

    if betreff.is_empty() {
        return Err(AmexCsvError::MissingBetreff { row: row_num });
    }

    let posted_date = parse_german_date(&row.datum, row_num)?;
    let amount = parse_german_amount(&row.betrag, row_num)?;

    // Amex CSV: positive = charge, negative = credit (payment/refund).
    // Our domain: negative = money out, positive = money in.
    let amount = -amount;

    let fx = parse_fx_details(&row.weitere_details);

    let mut remittance = Vec::new();
    if !row.weitere_details.is_empty() {
        remittance.push(row.weitere_details.clone());
    }
    if !row.kategorie.is_empty() {
        remittance.push(row.kategorie.clone());
    }
    if !row.karteninhaber.is_empty() {
        remittance.push(format!("Karteninhaber: {}", row.karteninhaber));
    }

    Ok(Transaction {
        provider_transaction_id: betreff.to_owned(),
        amount,
        currency: "EUR".to_owned(),
        merchant_name: row.beschreibung.clone(),
        remittance_information: remittance,
        posted_date,
        counterparty_name: None,
        counterparty_iban: None,
        counterparty_bic: None,
        bank_transaction_code: None,
        merchant_category_code: None,
        original_amount: fx.as_ref().map(|f| f.foreign_amount),
        original_currency: fx.as_ref().map(|f| f.currency.clone()),
        bank_transaction_code_code: None,
        bank_transaction_code_sub_code: None,
        exchange_rate: fx.as_ref().map(|f| f.exchange_rate.clone()),
        exchange_rate_unit_currency: fx.as_ref().map(|_| "EUR".to_owned()),
        exchange_rate_type: None,
        exchange_rate_contract_id: None,
        reference_number: None,
        reference_number_schema: None,
        note: None,
        balance_after_transaction: None,
        balance_after_transaction_currency: None,
        creditor_account_additional_id: None,
        debtor_account_additional_id: None,
    })
}

/// Parse `DD/MM/YYYY` dates from the Amex CSV.
fn parse_german_date(s: &str, row_num: usize) -> Result<NaiveDate, AmexCsvError> {
    NaiveDate::parse_from_str(s.trim(), "%d/%m/%Y").map_err(|_| AmexCsvError::InvalidDate {
        row: row_num,
        value: s.to_owned(),
    })
}

/// Parse German-locale decimal amounts: `"1.234,56"` → `1234.56`, `"42,86"` → `42.86`.
fn parse_german_amount(s: &str, row_num: usize) -> Result<Decimal, AmexCsvError> {
    // Strip thousands separator (`.`), replace decimal comma with dot
    let normalized = s.trim().replace('.', "").replace(',', ".");
    Decimal::from_str(&normalized).map_err(|_| AmexCsvError::InvalidAmount {
        row: row_num,
        value: s.to_owned(),
    })
}

/// Parse FX details from the `Weitere Details` field.
///
/// Format: `"Foreign Spend Amount: 383.00 Hong Kong Dollar Commission Amount: 0,84 Currency Exchange Rate: 9.1147"`
///
/// Foreign amounts use English notation (dots for decimals, commas for thousands).
/// Integer amounts (e.g. Yen) have no decimal point: `"302 Japanische Yen"`.
/// Comma-thousands in foreign amounts: `"6,150 Japanische Yen"`, `"1,030.00 Chinesischer Renminbi"`.
fn parse_fx_details(details: &str) -> Option<FxInfo> {
    let caps = FX_RE.captures(details)?;

    let raw_amount = caps.get(1)?.as_str();
    let currency_name = caps.get(2)?.as_str().trim();
    let rate = caps.get(3)?.as_str();

    // Determine if this is an integer-amount currency (like JPY) or decimal.
    // If the raw amount contains a dot, it's decimal notation with possible comma thousands.
    // If it has only commas, those are thousands separators (no decimal point).
    let amount_str = if raw_amount.contains('.') {
        // Has decimal point: commas are thousands separators (English notation)
        // e.g. "1,030.00" → "1030.00"
        raw_amount.replace(',', "")
    } else {
        // No decimal point: commas are thousands separators for integer amounts
        // e.g. "6,150" → "6150", "302" → "302"
        raw_amount.replace(',', "")
    };

    let foreign_amount = Decimal::from_str(&amount_str).ok()?;

    Some(FxInfo {
        foreign_amount,
        currency: currency_name.to_owned(),
        exchange_rate: rate.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn header() -> &'static str {
        "Datum,Beschreibung,Karteninhaber,Konto #,Betrag,Weitere Details,Erscheint auf Ihrer Abrechnung als,Adresse,Stadt,PLZ,Land,Betreff,Kategorie"
    }

    fn csv(rows: &[&str]) -> String {
        let mut lines = vec![header().to_owned()];
        lines.extend(rows.iter().map(|r| (*r).to_owned()));
        lines.join("\n")
    }

    // --- Edge case 1: German decimal amounts ---
    #[test]
    fn german_decimal_amount() {
        let input = csv(&[
            r#"11/03/2026,BBMSL*Tim Ho Wan Dim Su HK,HARRIET J JEEVES,-31018,"42,86","Foreign Spend Amount: 383.00 Hong Kong Dollar Commission Amount: 0,84 Currency Exchange Rate: 9.1147",BBMSL*Tim Ho Wan Dim Su HK,"GF KA WUI BLDG SSP
HONG KONG",,,HONG KONG,'AT260700036000010312575',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns.len(), 1);
        // Positive Amex amount becomes negative (spending)
        assert_eq!(txns[0].amount, dec!(-42.86));
    }

    // --- Edge case 2: Thousands separator ---
    #[test]
    fn thousands_separator() {
        let input = csv(&[
            r#"03/03/2026,ZAHLUNG/ÜBERWEISUNG ERHALTEN BESTEN DANK,SAMUEL JEEVES,-31000,"-7857,08",,ZAHLUNG/ÜBERWEISUNG ERHALTEN BESTEN DANK,,,,,'10000000010228999645130',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        // Negative CSV amount (payment received) → positive in our domain
        assert_eq!(txns[0].amount, dec!(7857.08));
    }

    // --- Edge case 3: Negative amount (refund) ---
    #[test]
    fn refund_becomes_positive() {
        let input = csv(&[
            r#"04/03/2026,HM.COM                  HAMBURG,HARRIET J JEEVES,-31018,"-76,01",,HM.COM                  HAMBURG,"RUNGEDAMM 38
HAMBURG",,21035,GERMANY,'AT260630052000010294564',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        // Negative CSV amount (refund) → positive in our domain
        assert_eq!(txns[0].amount, dec!(76.01));
    }

    // --- Edge case 4: Japanese Yen (no decimals) ---
    #[test]
    fn yen_integer_amount() {
        let input = csv(&[
            r#"05/03/2026,RIEVENHOUSE TOKYO NIHON TOKYO,HARRIET J JEEVES,-31018,"1,69","Foreign Spend Amount: 302 Japanische Yen Commission Amount: 0,03 Currency Exchange Rate: 181.9277",RIEVENHOUSE TOKYO NIHON TOKYO,"WALSALL RUGBY FOOTBALL CLUB
DELVES ROAD
CHUO-KU",,WS1 3JY,JAPAN,'AT260670036000010408585',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].original_amount, Some(dec!(302)));
        assert_eq!(txns[0].original_currency.as_deref(), Some("Japanische Yen"));
        assert_eq!(txns[0].exchange_rate.as_deref(), Some("181.9277"));
    }

    // --- Edge case 5a: FX amounts with comma thousands (Yen) ---
    #[test]
    fn fx_comma_thousands_yen() {
        let input = csv(&[
            r#"05/03/2026,YABATON TOKYOGINZA      TOKYO,SAMUEL JEEVES,-31000,"34,46","Foreign Spend Amount: 6,150 Japanische Yen Commission Amount: 0,68 Currency Exchange Rate: 182.0603",YABATON TOKYOGINZA      TOKYO,"TAHACHAL
SOALTEEMODE
CHUO-KU",,44600,JAPAN,'AT260670036000010408583',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].original_amount, Some(dec!(6150)));
    }

    // --- Edge case 5b: FX amounts with dot-decimal (CNY) ---
    #[test]
    fn fx_dot_decimal_cny() {
        let input = csv(&[
            r#"10/03/2026,Alipay China            Shanghai,HARRIET J JEEVES,-31018,"131,37","Foreign Spend Amount: 1,030.00 Chinesischer Renminbi Commission Amount: 2,58 Currency Exchange Rate: 7.9975",Alipay China            Shanghai,,,,,'AT260700036000010291011',Miscellaneous-Other"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].original_amount, Some(dec!(1030.00)));
        assert_eq!(
            txns[0].original_currency.as_deref(),
            Some("Chinesischer Renminbi")
        );
    }

    // --- Edge case 6: Mixed locale in FX field ---
    #[test]
    fn mixed_locale_fx() {
        let input = csv(&[
            r#"11/03/2026,BBMSL*Tim Ho Wan Dim Su HK,HARRIET J JEEVES,-31018,"42,86","Foreign Spend Amount: 383.00 Hong Kong Dollar Commission Amount: 0,84 Currency Exchange Rate: 9.1147",BBMSL*Tim Ho Wan Dim Su HK,"GF KA WUI BLDG SSP
HONG KONG",,,HONG KONG,'AT260700036000010312575',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].original_amount, Some(dec!(383.00)));
        assert_eq!(txns[0].exchange_rate.as_deref(), Some("9.1147"));
    }

    // --- Edge case 7: Multiline quoted addresses ---
    #[test]
    fn multiline_address() {
        let input = csv(&[
            r#"10/03/2026,KPAY*HO KEE REPACKET 00 HONG KONG,HARRIET J JEEVES,-31018,"7,78","Foreign Spend Amount: 69.00 Hong Kong Dollar Commission Amount: 0,15 Currency Exchange Rate: 9.0432",KPAY*HO KEE REPACKET 00 HONG KONG,"GROUND FLOOR SHOP 60 RUSSELL STREET CAUS
EWAY BAY HK
CAUSEWAY BAY
HONGKONG",,000000,HONG KONG,'AT260690043000010314602',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].amount, dec!(-7.78));
        assert_eq!(txns[0].provider_transaction_id, "AT260690043000010314602");
    }

    // --- Edge case 8: Garbled unicode ---
    #[test]
    fn garbled_unicode() {
        let input = csv(&[
            r#"05/03/2026,FAMILY MART             *,HARRIET J JEEVES,-31018,"1,05","Foreign Spend Amount: 189 Japanische Yen Commission Amount: 0,02 Currency Exchange Rate: 183.4951",FAMILY MART             *,"ncgÔcn vonh
ÇÎæc[ 3-1-21
MSB TAMACHI
jukÈm-ÇÔ]jä-S 9F
SHIBAURA",,108-0023,JAPAN,'AT260650055000010318346',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].merchant_name, "FAMILY MART             *");
    }

    // --- Edge case 9: Apostrophes in merchant names ---
    #[test]
    fn apostrophe_in_merchant_name() {
        let input = csv(&[
            r#"08/03/2026,P.J. O'BRIEN'S - SYDNEY SYDNEY,SAMUEL JEEVES,-31000,"17,65","Foreign Spend Amount: 28.39 Australische Dollars Commission Amount: 0,35 Currency Exchange Rate: 1.641",P.J. O'BRIEN'S - SYDNEY SYDNEY,"57 KING ST
SYDNEY",,2000,AUSTRALIA,'AT260680031000010212454',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].merchant_name, "P.J. O'BRIEN'S - SYDNEY SYDNEY");
    }

    // --- Edge case 10: Duplicate merchant same day, different Betreff ---
    #[test]
    fn duplicate_merchant_different_betreff() {
        let input = csv(&[
            r#"04/03/2026,SUICA KEITAIKESSAI      TOKYO,HARRIET J JEEVES,-31018,"5,59","Foreign Spend Amount: 1,000 Japanische Yen Commission Amount: 0,11 Currency Exchange Rate: 182.4817",SUICA KEITAIKESSAI      TOKYO,"RYNEK JEZYCKI 2
SHIBUYA-KU",,60-847,JAPAN,'AT260670036000010408592',"#,
            r#"04/03/2026,SUICA KEITAIKESSAI      TOKYO,SAMUEL JEEVES,-31000,"5,59","Foreign Spend Amount: 1,000 Japanische Yen Commission Amount: 0,11 Currency Exchange Rate: 182.4817",SUICA KEITAIKESSAI      TOKYO,"RYNEK JEZYCKI 2
SHIBUYA-KU",,60-847,JAPAN,'AT260670036000010408593',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns.len(), 2);
        assert_ne!(
            txns[0].provider_transaction_id,
            txns[1].provider_transaction_id
        );
    }

    // --- Edge case 11: Domestic transaction (no FX data) ---
    #[test]
    fn domestic_no_fx() {
        let input = csv(&[
            r#"10/03/2026,APPLE.COM/BILL          HOLLYHILL,HARRIET J JEEVES,-31018,"2,99",,APPLE.COM/BILL          HOLLYHILL,"SUBMISSIONS EURO
8/10 MATHIAS HARDT
LUXEMBURG",,L-0717,LUXEMBOURG,'AT260690074000010120616',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].original_amount, None);
        assert_eq!(txns[0].original_currency, None);
        assert_eq!(txns[0].exchange_rate, None);
    }

    // --- Edge case 12: Betreff with surrounding quotes ---
    #[test]
    fn betreff_strips_single_quotes() {
        let input = csv(&[
            r#"11/03/2026,BBMSL*Tim Ho Wan Dim Su HK,HARRIET J JEEVES,-31018,"42,86","Foreign Spend Amount: 383.00 Hong Kong Dollar Commission Amount: 0,84 Currency Exchange Rate: 9.1147",BBMSL*Tim Ho Wan Dim Su HK,"GF KA WUI BLDG SSP
HONG KONG",,,HONG KONG,'AT260700036000010312575',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].provider_transaction_id, "AT260700036000010312575");
    }

    // --- Edge case 13: Embedded double quotes in addresses ---
    #[test]
    fn embedded_double_quotes() {
        let input = csv(&[
            r#"11/03/2026,MONDIFY - CENTRAL MARKE HONG KONG,HARRIET J JEEVES,-31018,"10,96","Foreign Spend Amount: 98.00 Hong Kong Dollar Commission Amount: 0,21 Currency Exchange Rate: 9.1162",MONDIFY - CENTRAL MARKE HONG KONG,"""UNIT B, 12/F.""
HANG SENG CAUSEWAY BAY BUILDING
28 YEE WO STREET
CAUSEWAY BAY",,,HONG KONG,'AT260700036000010279392',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].amount, dec!(-10.96));
    }

    // --- Edge case 14: Empty optional fields ---
    #[test]
    fn empty_optional_fields() {
        let input = csv(&[
            r#"10/03/2026,Alipay China            Shanghai,HARRIET J JEEVES,-31018,"131,37","Foreign Spend Amount: 1,030.00 Chinesischer Renminbi Commission Amount: 2,58 Currency Exchange Rate: 7.9975",Alipay China            Shanghai,,,,,'AT260700036000010291011',Miscellaneous-Other"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns.len(), 1);
    }

    // --- Edge case 15: Re-import deduplication (parser returns same IDs) ---
    #[test]
    fn reimport_produces_same_ids() {
        let row = r#"10/03/2026,APPLE.COM/BILL          HOLLYHILL,HARRIET J JEEVES,-31018,"2,99",,APPLE.COM/BILL          HOLLYHILL,"SUBMISSIONS EURO
8/10 MATHIAS HARDT
LUXEMBURG",,L-0717,LUXEMBOURG,'AT260690074000010120616',"#;
        let input = csv(&[row]);
        let first = parse_amex_csv(&input).unwrap();
        let second = parse_amex_csv(&input).unwrap();
        assert_eq!(
            first[0].provider_transaction_id,
            second[0].provider_transaction_id
        );
    }

    // --- Empty CSV ---
    #[test]
    fn empty_csv_returns_error() {
        let input = header().to_owned();
        let err = parse_amex_csv(&input).unwrap_err();
        assert!(matches!(err, AmexCsvError::Empty));
    }

    // --- Large hotel amount with comma thousands in FX ---
    #[test]
    fn large_yen_amount_with_comma_thousands() {
        let input = csv(&[
            r#"03/03/2026,THE SQUARE HOTEL GINZA  *,SAMUEL JEEVES,-31000,"424,78","Foreign Spend Amount: 75,956 Japanische Yen Commission Amount: 8,33 Currency Exchange Rate: 182.3892",THE SQUARE HOTEL GINZA  *,"ncgÔcn kÒcech
gæ]Ææ 2-11-6
GINZA",,104-0061,JAPAN,'AT260630052000010321915',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert_eq!(txns[0].original_amount, Some(dec!(75956)));
    }

    // --- Payment becomes positive ---
    #[test]
    fn payment_becomes_positive() {
        let input = csv(&[
            r#"03/03/2026,ZAHLUNG/ÜBERWEISUNG ERHALTEN BESTEN DANK,SAMUEL JEEVES,-31000,"-7857,08",,ZAHLUNG/ÜBERWEISUNG ERHALTEN BESTEN DANK,,,,,'10000000010228999645130',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        // Negative CSV amount (payment received) → positive in our domain
        assert_eq!(txns[0].amount, dec!(7857.08));
    }

    // --- Kategorie in remittance info ---
    #[test]
    fn kategorie_in_remittance() {
        let input = csv(&[
            r#"10/03/2026,Alipay China            Shanghai,HARRIET J JEEVES,-31018,"131,37","Foreign Spend Amount: 1,030.00 Chinesischer Renminbi Commission Amount: 2,58 Currency Exchange Rate: 7.9975",Alipay China            Shanghai,,,,,'AT260700036000010291011',Miscellaneous-Other"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert!(
            txns[0]
                .remittance_information
                .iter()
                .any(|r| r == "Miscellaneous-Other")
        );
    }

    // --- Karteninhaber in remittance info ---
    #[test]
    fn karteninhaber_in_remittance() {
        let input = csv(&[
            r#"10/03/2026,APPLE.COM/BILL          HOLLYHILL,HARRIET J JEEVES,-31018,"2,99",,APPLE.COM/BILL          HOLLYHILL,"SUBMISSIONS EURO
8/10 MATHIAS HARDT
LUXEMBURG",,L-0717,LUXEMBOURG,'AT260690074000010120616',"#,
        ]);
        let txns = parse_amex_csv(&input).unwrap();
        assert!(
            txns[0]
                .remittance_information
                .iter()
                .any(|r| r == "Karteninhaber: HARRIET J JEEVES")
        );
    }

    // --- parse_german_amount unit tests ---
    #[test]
    fn parse_amount_simple() {
        assert_eq!(parse_german_amount("42,86", 1).unwrap(), dec!(42.86));
    }

    #[test]
    fn parse_amount_thousands() {
        assert_eq!(parse_german_amount("1.234,56", 1).unwrap(), dec!(1234.56));
    }

    #[test]
    fn parse_amount_negative() {
        assert_eq!(parse_german_amount("-76,01", 1).unwrap(), dec!(-76.01));
    }

    #[test]
    fn parse_amount_large_negative() {
        assert_eq!(parse_german_amount("-7857,08", 1).unwrap(), dec!(-7857.08));
    }

    // --- parse_german_date unit tests ---
    #[test]
    fn parse_date_valid() {
        assert_eq!(
            parse_german_date("11/03/2026", 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 11).unwrap()
        );
    }

    #[test]
    fn parse_date_invalid() {
        assert!(parse_german_date("2026-03-11", 1).is_err());
    }

    // --- parse_fx_details unit tests ---
    #[test]
    fn fx_hkd() {
        let fx = parse_fx_details(
            "Foreign Spend Amount: 383.00 Hong Kong Dollar Commission Amount: 0,84 Currency Exchange Rate: 9.1147",
        )
        .unwrap();
        assert_eq!(fx.foreign_amount, dec!(383.00));
        assert_eq!(fx.currency, "Hong Kong Dollar");
        assert_eq!(fx.exchange_rate, "9.1147");
    }

    #[test]
    fn fx_yen_integer() {
        let fx = parse_fx_details(
            "Foreign Spend Amount: 302 Japanische Yen Commission Amount: 0,03 Currency Exchange Rate: 181.9277",
        )
        .unwrap();
        assert_eq!(fx.foreign_amount, dec!(302));
        assert_eq!(fx.currency, "Japanische Yen");
    }

    #[test]
    fn fx_yen_comma_thousands() {
        let fx = parse_fx_details(
            "Foreign Spend Amount: 6,150 Japanische Yen Commission Amount: 0,68 Currency Exchange Rate: 182.0603",
        )
        .unwrap();
        assert_eq!(fx.foreign_amount, dec!(6150));
    }

    #[test]
    fn fx_cny_dot_decimal() {
        let fx = parse_fx_details(
            "Foreign Spend Amount: 1,030.00 Chinesischer Renminbi Commission Amount: 2,58 Currency Exchange Rate: 7.9975",
        )
        .unwrap();
        assert_eq!(fx.foreign_amount, dec!(1030.00));
        assert_eq!(fx.currency, "Chinesischer Renminbi");
    }

    #[test]
    fn fx_empty_returns_none() {
        assert!(parse_fx_details("").is_none());
    }

    #[test]
    fn fx_large_yen_hotel() {
        let fx = parse_fx_details(
            "Foreign Spend Amount: 75,956 Japanische Yen Commission Amount: 8,33 Currency Exchange Rate: 182.3892",
        )
        .unwrap();
        assert_eq!(fx.foreign_amount, dec!(75956));
    }
}
