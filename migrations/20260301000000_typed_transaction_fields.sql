-- Reverse the join(" / ") encoding and store remittance info as a native array.
--
-- Existing descriptions are single remittance segments from the Enable Banking API
-- (the API returned them as single-element arrays). We wrap each non-empty description
-- as a single-element TEXT[] array. Descriptions containing " / " within the text are
-- preserved intact (they are loan payment details, not segment separators).
ALTER TABLE transactions ADD COLUMN remittance_information TEXT[] NOT NULL DEFAULT '{}';
UPDATE transactions SET remittance_information = CASE
    WHEN description = '' THEN '{}'
    ELSE ARRAY[description]
END;
ALTER TABLE transactions DROP COLUMN description;

-- ISO 18245 MCC code (e.g. "5411" = grocery). Only present for card transactions.
ALTER TABLE transactions ADD COLUMN merchant_category_code TEXT;

-- ISO 20022 domain code (e.g. "PMNT" for payments).
ALTER TABLE transactions ADD COLUMN bank_transaction_code_code TEXT;

-- ISO 20022 sub-family code (e.g. "ICDT-STDO").
ALTER TABLE transactions ADD COLUMN bank_transaction_code_sub_code TEXT;

-- Actual FX rate applied (e.g. "1.0856"). Stored as text to preserve bank precision.
ALTER TABLE transactions ADD COLUMN exchange_rate TEXT;

-- ISO 4217 currency code in which the exchange rate is expressed.
ALTER TABLE transactions ADD COLUMN exchange_rate_unit_currency TEXT;

-- FX rate type: AGRD (agreed/contract), SALE, or SPOT.
ALTER TABLE transactions ADD COLUMN exchange_rate_type TEXT;

-- FX contract reference when rate_type is AGRD.
ALTER TABLE transactions ADD COLUMN exchange_rate_contract_id TEXT;

-- Structured payment reference (e.g. "RF07850352502356628678117").
ALTER TABLE transactions ADD COLUMN reference_number TEXT;

-- Scheme of the reference number: BERF, FIRF, INTL, NORF, SDDM, SEBG.
ALTER TABLE transactions ADD COLUMN reference_number_schema TEXT;

-- Internal note made by PSU, distinct from remittance info.
ALTER TABLE transactions ADD COLUMN note TEXT;

-- Account balance after this transaction.
ALTER TABLE transactions ADD COLUMN balance_after_transaction NUMERIC;

-- Currency of the balance after transaction.
ALTER TABLE transactions ADD COLUMN balance_after_transaction_currency TEXT;

-- Non-IBAN creditor account IDs: JSON array of {identification, scheme_name, issuer}.
ALTER TABLE transactions ADD COLUMN creditor_account_additional_id JSONB;

-- Non-IBAN debtor account IDs: JSON array of {identification, scheme_name, issuer}.
ALTER TABLE transactions ADD COLUMN debtor_account_additional_id JSONB;
