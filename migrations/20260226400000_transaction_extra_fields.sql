ALTER TABLE transactions ADD COLUMN counterparty_name TEXT;
ALTER TABLE transactions ADD COLUMN counterparty_iban TEXT;
ALTER TABLE transactions ADD COLUMN counterparty_bic TEXT;
ALTER TABLE transactions ADD COLUMN bank_transaction_code TEXT;
