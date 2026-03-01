use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;

use super::client::Client;
use super::types::{ApiTransaction, Balance, SessionAccount};
use crate::bank::{Account, AccountBalance, AccountId, BankProvider, Transaction};
use crate::error::ProviderError;

/// A `BankProvider` backed by an authenticated Enable Banking session.
///
/// Constructed with an already-resolved session — the OAuth redirect dance
/// is handled by `EnableBankingAuth` before this struct is created.
pub struct EnableBankingProvider {
    client: Client,
    /// Retained for future session refresh/revocation
    _session_id: String,
    accounts: Vec<SessionAccount>,
}

impl EnableBankingProvider {
    #[must_use]
    pub fn new(client: Client, session_id: String, accounts: Vec<SessionAccount>) -> Self {
        Self {
            client,
            _session_id: session_id,
            accounts,
        }
    }
}

impl BankProvider for EnableBankingProvider {
    async fn list_accounts(&self) -> Result<Vec<Account>, ProviderError> {
        Ok(self.accounts.iter().map(convert_account).collect())
    }

    async fn fetch_transactions(
        &self,
        account_id: &AccountId,
        since: Option<NaiveDate>,
    ) -> Result<Vec<Transaction>, ProviderError> {
        let today = Utc::now().date_naive();
        let date_from = since.or_else(|| Some(today - chrono::Duration::days(90)));
        let date_to = Some(today);
        let mut all_transactions = Vec::new();
        let mut continuation_key: Option<String> = None;
        let mut pages: u32 = 0;
        let mut skipped_pending: u32 = 0;

        loop {
            let response = self
                .client
                .get_transactions(
                    account_id.as_str(),
                    date_from,
                    date_to,
                    continuation_key.as_deref(),
                )
                .await?;

            pages += 1;
            for api_txn in response.transactions {
                match convert_transaction(&api_txn)? {
                    Some(txn) => all_transactions.push(txn),
                    None => skipped_pending += 1,
                }
            }

            match response.continuation_key {
                Some(key) if !key.is_empty() => continuation_key = Some(key),
                _ => break,
            }
        }

        tracing::info!(
            account_id = account_id.as_str(),
            since = ?since,
            pages,
            fetched = all_transactions.len(),
            skipped_pending,
            "Enable Banking fetch_transactions complete"
        );

        Ok(all_transactions)
    }

    async fn get_balances(&self, account_id: &AccountId) -> Result<AccountBalance, ProviderError> {
        let response = self.client.get_balances(account_id.as_str()).await?;

        let mut available: Option<(Decimal, String)> = None;
        let mut current: Option<(Decimal, String)> = None;

        for balance in &response.balances {
            match balance.balance_type.as_str() {
                // CLAV = closing available, ITAV = interim available
                "CLAV" => {
                    available = Some((
                        balance.balance_amount.amount,
                        balance.balance_amount.currency.clone(),
                    ));
                }
                "ITAV" => {
                    if available.is_none() {
                        available = Some((
                            balance.balance_amount.amount,
                            balance.balance_amount.currency.clone(),
                        ));
                    }
                }
                // CLBD = closing booked, ITBD = interim booked
                "CLBD" => {
                    current = Some((
                        balance.balance_amount.amount,
                        balance.balance_amount.currency.clone(),
                    ));
                }
                "ITBD" => {
                    if current.is_none() {
                        current = Some((
                            balance.balance_amount.amount,
                            balance.balance_amount.currency.clone(),
                        ));
                    }
                }
                _ => {}
            }
        }

        let fallback = first_balance_amount(&response.balances);

        let (avail_amount, currency) = available
            .or_else(|| current.clone())
            .or_else(|| fallback.clone())
            .ok_or_else(|| {
                ProviderError::Other(format!(
                    "no balances returned for account {}",
                    account_id.as_str()
                ))
            })?;

        let (curr_amount, _) = current
            .or(fallback)
            .unwrap_or((avail_amount, currency.clone()));

        Ok(AccountBalance {
            account_id: account_id.as_str().to_owned(),
            available: avail_amount,
            current: curr_amount,
            currency,
        })
    }
}

fn first_balance_amount(balances: &[Balance]) -> Option<(Decimal, String)> {
    balances
        .first()
        .map(|b| (b.balance_amount.amount, b.balance_amount.currency.clone()))
}

fn convert_account(acct: &SessionAccount) -> Account {
    let account_type = acct
        .cash_account_type
        .as_deref()
        .map_or("checking", map_account_type)
        .to_owned();

    let iban = acct.account_id.as_ref().and_then(|id| id.iban.clone());

    let institution = acct
        .account_servicer
        .as_ref()
        .and_then(|s| s.name.clone())
        .unwrap_or_default();

    Account {
        provider_account_id: acct.uid.clone(),
        name: acct
            .name
            .clone()
            .or_else(|| acct.product.clone())
            .or(iban)
            .unwrap_or_else(|| acct.uid.clone()),
        institution,
        account_type,
        currency: acct.currency.clone().unwrap_or_else(|| "EUR".to_owned()),
    }
}

fn map_account_type(cash_account_type: &str) -> &str {
    match cash_account_type {
        "SVGS" => "savings",
        "CARD" => "credit_card",
        "LOAN" => "loan",
        _ => "checking",
    }
}

/// Convert an API transaction to our domain `Transaction`.
///
/// Returns `Ok(None)` for pending transactions (PDNG status) which are skipped.
fn convert_transaction(api: &ApiTransaction) -> Result<Option<Transaction>, ProviderError> {
    if api.status == "PDNG" {
        return Ok(None);
    }

    let id = api
        .transaction_id
        .as_ref()
        .or(api.entry_reference.as_ref())
        .ok_or_else(|| {
            ProviderError::Other("transaction missing both id and entry_reference".to_owned())
        })?
        .clone();

    let posted_date = api
        .booking_date
        .or(api.value_date)
        .or(api.transaction_date)
        .ok_or_else(|| ProviderError::Other(format!("transaction {id} has no date")))?;

    let raw_amount = api.transaction_amount.amount;
    let amount = match api.credit_debit_indicator.as_str() {
        "DBIT" => -raw_amount.abs(),
        _ => raw_amount.abs(),
    };

    let (merchant_name, counterparty_name) = extract_names(api);

    let fx = extract_fx(api);
    let (counterparty_iban, counterparty_bic) = extract_counterparty_bank(api);
    let bank_transaction_code = api
        .bank_transaction_code
        .as_ref()
        .and_then(|b| b.description.clone());
    let bank_transaction_code_code = api
        .bank_transaction_code
        .as_ref()
        .and_then(|b| b.code.clone());
    let bank_transaction_code_sub_code = api
        .bank_transaction_code
        .as_ref()
        .and_then(|b| b.sub_code.clone());

    let (balance_after_transaction, balance_after_transaction_currency) = api
        .balance_after_transaction
        .as_ref()
        .map_or((None, None), |a| (Some(a.amount), Some(a.currency.clone())));

    let creditor_account_additional_id = api
        .creditor_account_additional_identification
        .as_ref()
        .map(|ids| serde_json::to_value(ids).unwrap_or_default());
    let debtor_account_additional_id = api
        .debtor_account_additional_identification
        .as_ref()
        .map(|ids| serde_json::to_value(ids).unwrap_or_default());

    Ok(Some(Transaction {
        provider_transaction_id: id,
        amount,
        currency: api.transaction_amount.currency.clone(),
        merchant_name,
        remittance_information: api.remittance_information.clone(),
        posted_date,
        counterparty_name,
        counterparty_iban,
        counterparty_bic,
        bank_transaction_code,
        merchant_category_code: api.merchant_category_code.clone(),
        original_amount: fx.original_amount,
        original_currency: fx.original_currency,
        bank_transaction_code_code,
        bank_transaction_code_sub_code,
        exchange_rate: fx.exchange_rate,
        exchange_rate_unit_currency: fx.unit_currency,
        exchange_rate_type: fx.rate_type,
        exchange_rate_contract_id: fx.contract_id,
        reference_number: api.reference_number.clone(),
        reference_number_schema: api.reference_number_schema.clone(),
        note: api.note.clone(),
        balance_after_transaction,
        balance_after_transaction_currency,
        creditor_account_additional_id,
        debtor_account_additional_id,
    }))
}

/// For debits, the creditor is the merchant; for credits, the debtor is the merchant.
/// The counterparty is the other party.
fn extract_names(api: &ApiTransaction) -> (String, Option<String>) {
    let is_debit = api.credit_debit_indicator == "DBIT";

    let creditor_name = api.creditor.as_ref().and_then(|p| p.name.clone());
    let debtor_name = api.debtor.as_ref().and_then(|p| p.name.clone());

    let merchant = if is_debit {
        creditor_name.clone()
    } else {
        debtor_name.clone()
    };

    let counterparty = if is_debit { debtor_name } else { creditor_name };

    let merchant_name = merchant
        .or_else(|| api.remittance_information.first().cloned())
        .unwrap_or_default();

    (merchant_name, counterparty)
}

/// For debits, the creditor is the merchant; for credits, the debtor is the sender.
/// Extract the counterparty's IBAN and BIC accordingly.
fn extract_counterparty_bank(api: &ApiTransaction) -> (Option<String>, Option<String>) {
    let is_debit = api.credit_debit_indicator == "DBIT";

    let iban = if is_debit {
        api.creditor_account.as_ref().and_then(|a| a.iban.clone())
    } else {
        api.debtor_account.as_ref().and_then(|a| a.iban.clone())
    };

    let bic = if is_debit {
        api.creditor_agent.as_ref().and_then(|a| a.bic_fi.clone())
    } else {
        api.debtor_agent.as_ref().and_then(|a| a.bic_fi.clone())
    };

    (iban, bic)
}

struct FxFields {
    original_amount: Option<Decimal>,
    original_currency: Option<String>,
    exchange_rate: Option<String>,
    unit_currency: Option<String>,
    rate_type: Option<String>,
    contract_id: Option<String>,
}

fn extract_fx(api: &ApiTransaction) -> FxFields {
    let fx = api.exchange_rate.as_ref();
    let instructed = fx.and_then(|r| r.instructed_amount.as_ref());

    FxFields {
        original_amount: instructed.map(|a| a.amount),
        original_currency: instructed.map(|a| a.currency.clone()),
        exchange_rate: fx.and_then(|r| r.exchange_rate.clone()),
        unit_currency: fx.and_then(|r| r.unit_currency.clone()),
        rate_type: fx.and_then(|r| r.rate_type.clone()),
        contract_id: fx.and_then(|r| r.contract_identification.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enable_banking::types::{
        AccountIdentificationTxn, AgentIdentification, Amount, BankTransactionCode, ExchangeRate,
        PartyIdentification,
    };
    use rust_decimal_macros::dec;

    fn base_api_txn() -> ApiTransaction {
        ApiTransaction {
            transaction_id: Some("txn-001".to_owned()),
            entry_reference: None,
            status: "BOOK".to_owned(),
            credit_debit_indicator: "DBIT".to_owned(),
            transaction_amount: Amount {
                amount: dec!(42.50),
                currency: "EUR".to_owned(),
            },
            booking_date: Some(NaiveDate::from_ymd_opt(2025, 3, 15).unwrap()),
            value_date: None,
            transaction_date: None,
            remittance_information: vec![],
            creditor: Some(PartyIdentification {
                name: Some("Coffee Shop".to_owned()),
            }),
            debtor: None,
            merchant_category_code: Some("5411".to_owned()),
            exchange_rate: None,
            creditor_account: None,
            debtor_account: None,
            creditor_agent: None,
            debtor_agent: None,
            bank_transaction_code: None,
            balance_after_transaction: None,
            reference_number: None,
            reference_number_schema: None,
            note: None,
            debtor_account_additional_identification: None,
            creditor_account_additional_identification: None,
        }
    }

    #[test]
    fn debit_produces_negative_amount() {
        let api = base_api_txn();
        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.amount, dec!(-42.50));
    }

    #[test]
    fn credit_produces_positive_amount() {
        let mut api = base_api_txn();
        api.credit_debit_indicator = "CRDT".to_owned();
        api.debtor = Some(PartyIdentification {
            name: Some("Employer Inc".to_owned()),
        });
        api.creditor = None;

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.amount, dec!(42.50));
        assert_eq!(txn.merchant_name, "Employer Inc");
    }

    #[test]
    fn date_fallback_to_value_date() {
        let mut api = base_api_txn();
        api.booking_date = None;
        api.value_date = Some(NaiveDate::from_ymd_opt(2025, 3, 14).unwrap());

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(
            txn.posted_date,
            NaiveDate::from_ymd_opt(2025, 3, 14).unwrap()
        );
    }

    #[test]
    fn date_fallback_to_transaction_date() {
        let mut api = base_api_txn();
        api.booking_date = None;
        api.value_date = None;
        api.transaction_date = Some(NaiveDate::from_ymd_opt(2025, 3, 13).unwrap());

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(
            txn.posted_date,
            NaiveDate::from_ymd_opt(2025, 3, 13).unwrap()
        );
    }

    #[test]
    fn missing_all_dates_is_error() {
        let mut api = base_api_txn();
        api.booking_date = None;
        api.value_date = None;
        api.transaction_date = None;

        let result = convert_transaction(&api);
        assert!(result.is_err());
    }

    #[test]
    fn missing_id_and_entry_reference_is_error() {
        let mut api = base_api_txn();
        api.transaction_id = None;
        api.entry_reference = None;

        let result = convert_transaction(&api);
        assert!(result.is_err());
    }

    #[test]
    fn entry_reference_used_as_fallback_id() {
        let mut api = base_api_txn();
        api.transaction_id = None;
        api.entry_reference = Some("ref-999".to_owned());

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.provider_transaction_id, "ref-999");
    }

    #[test]
    fn pending_transactions_are_skipped() {
        let mut api = base_api_txn();
        api.status = "PDNG".to_owned();

        let result = convert_transaction(&api).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn fx_amount_extracted() {
        let mut api = base_api_txn();
        api.exchange_rate = Some(ExchangeRate {
            instructed_amount: Some(Amount {
                amount: dec!(50.00),
                currency: "USD".to_owned(),
            }),
            unit_currency: None,
            exchange_rate: None,
            rate_type: None,
            contract_identification: None,
        });

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.original_amount, Some(dec!(50.00)));
        assert_eq!(txn.original_currency.as_deref(), Some("USD"));
    }

    #[test]
    fn debit_merchant_is_creditor() {
        let api = base_api_txn();
        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.merchant_name, "Coffee Shop");
        assert!(txn.counterparty_name.is_none());
    }

    #[test]
    fn credit_merchant_is_debtor() {
        let mut api = base_api_txn();
        api.credit_debit_indicator = "CRDT".to_owned();
        api.debtor = Some(PartyIdentification {
            name: Some("Employer".to_owned()),
        });
        api.creditor = Some(PartyIdentification {
            name: Some("Me".to_owned()),
        });

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.merchant_name, "Employer");
        assert_eq!(txn.counterparty_name.as_deref(), Some("Me"));
    }

    #[test]
    fn debit_counterparty_iban_bic_from_creditor() {
        let mut api = base_api_txn();
        api.creditor_account = Some(AccountIdentificationTxn {
            iban: Some("DE89370400440532013000".to_owned()),
        });
        api.creditor_agent = Some(AgentIdentification {
            bic_fi: Some("COBADEFFXXX".to_owned()),
        });

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(
            txn.counterparty_iban.as_deref(),
            Some("DE89370400440532013000")
        );
        assert_eq!(txn.counterparty_bic.as_deref(), Some("COBADEFFXXX"));
    }

    #[test]
    fn credit_counterparty_iban_bic_from_debtor() {
        let mut api = base_api_txn();
        api.credit_debit_indicator = "CRDT".to_owned();
        api.debtor = Some(PartyIdentification {
            name: Some("Employer".to_owned()),
        });
        api.debtor_account = Some(AccountIdentificationTxn {
            iban: Some("DE02120300000000202051".to_owned()),
        });
        api.debtor_agent = Some(AgentIdentification {
            bic_fi: Some("BYLADEM1001".to_owned()),
        });

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(
            txn.counterparty_iban.as_deref(),
            Some("DE02120300000000202051")
        );
        assert_eq!(txn.counterparty_bic.as_deref(), Some("BYLADEM1001"));
    }

    #[test]
    fn bank_transaction_code_extracted() {
        let mut api = base_api_txn();
        api.bank_transaction_code = Some(BankTransactionCode {
            description: Some("Gehalt/Rente".to_owned()),
            code: None,
            sub_code: None,
        });

        let txn = convert_transaction(&api).unwrap().unwrap();
        assert_eq!(txn.bank_transaction_code.as_deref(), Some("Gehalt/Rente"));
    }

    #[test]
    fn account_type_mapping() {
        assert_eq!(map_account_type("CACC"), "checking");
        assert_eq!(map_account_type("SVGS"), "savings");
        assert_eq!(map_account_type("CARD"), "credit_card");
        assert_eq!(map_account_type("LOAN"), "loan");
        assert_eq!(map_account_type("OTHR"), "checking");
    }

    #[test]
    fn balance_type_priority() {
        let balances = vec![
            Balance {
                balance_amount: Amount {
                    amount: dec!(100.00),
                    currency: "EUR".to_owned(),
                },
                balance_type: "ITAV".to_owned(),
            },
            Balance {
                balance_amount: Amount {
                    amount: dec!(200.00),
                    currency: "EUR".to_owned(),
                },
                balance_type: "CLAV".to_owned(),
            },
            Balance {
                balance_amount: Amount {
                    amount: dec!(150.00),
                    currency: "EUR".to_owned(),
                },
                balance_type: "ITBD".to_owned(),
            },
            Balance {
                balance_amount: Amount {
                    amount: dec!(175.00),
                    currency: "EUR".to_owned(),
                },
                balance_type: "CLBD".to_owned(),
            },
        ];

        // Simulate the balance selection logic
        let mut available: Option<Decimal> = None;
        let mut current: Option<Decimal> = None;

        for balance in &balances {
            match balance.balance_type.as_str() {
                "CLAV" => available = Some(balance.balance_amount.amount),
                "ITAV" if available.is_none() => {
                    available = Some(balance.balance_amount.amount);
                }
                "CLBD" => current = Some(balance.balance_amount.amount),
                "ITBD" if current.is_none() => {
                    current = Some(balance.balance_amount.amount);
                }
                _ => {}
            }
        }

        // CLAV takes priority over ITAV, CLBD takes priority over ITBD
        assert_eq!(available, Some(dec!(200.00)));
        assert_eq!(current, Some(dec!(175.00)));
    }
}
