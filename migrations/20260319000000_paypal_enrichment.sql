-- PayPal enrichment: accounts, transactions, items, and matches.

CREATE TABLE paypal_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    label TEXT NOT NULL,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    sandbox BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE paypal_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    paypal_account_id UUID NOT NULL REFERENCES paypal_accounts(id) ON DELETE CASCADE,
    paypal_transaction_id TEXT NOT NULL,
    transaction_date DATE NOT NULL,
    amount NUMERIC NOT NULL,
    currency TEXT NOT NULL DEFAULT 'EUR',
    merchant_name TEXT,
    event_code TEXT,
    status TEXT NOT NULL DEFAULT 'S',
    payer_email TEXT,
    payer_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (paypal_account_id, paypal_transaction_id)
);

CREATE INDEX idx_paypal_transactions_account ON paypal_transactions (paypal_account_id);
CREATE INDEX idx_paypal_transactions_date ON paypal_transactions (transaction_date);
CREATE INDEX idx_paypal_transactions_amount ON paypal_transactions (amount);

CREATE TABLE paypal_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    paypal_transaction_id UUID NOT NULL REFERENCES paypal_transactions(id) ON DELETE CASCADE,
    name TEXT,
    description TEXT,
    quantity TEXT,
    unit_price NUMERIC,
    unit_price_currency TEXT
);

CREATE INDEX idx_paypal_items_txn ON paypal_items (paypal_transaction_id);

CREATE TABLE paypal_matches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    paypal_transaction_id UUID NOT NULL REFERENCES paypal_transactions(id) ON DELETE CASCADE,
    bank_transaction_id UUID NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (paypal_transaction_id, bank_transaction_id)
);

CREATE INDEX idx_paypal_matches_bank_txn ON paypal_matches (bank_transaction_id);
