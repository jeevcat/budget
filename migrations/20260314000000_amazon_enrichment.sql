-- Amazon transaction enrichment tables

CREATE TABLE amazon_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    transaction_date DATE NOT NULL,
    amount NUMERIC NOT NULL,
    currency TEXT NOT NULL DEFAULT 'EUR',
    statement_descriptor TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'Charged',
    payment_method TEXT NOT NULL DEFAULT '',
    dedup_key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_amazon_transactions_date ON amazon_transactions (transaction_date);
CREATE INDEX idx_amazon_transactions_amount ON amazon_transactions (amount);

CREATE TABLE amazon_transaction_orders (
    amazon_transaction_id UUID NOT NULL REFERENCES amazon_transactions(id) ON DELETE CASCADE,
    order_id TEXT NOT NULL,
    PRIMARY KEY (amazon_transaction_id, order_id)
);

CREATE TABLE amazon_orders (
    order_id TEXT PRIMARY KEY,
    order_date DATE,
    grand_total NUMERIC,
    subtotal NUMERIC,
    shipping NUMERIC,
    vat NUMERIC,
    promotion NUMERIC,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE amazon_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id TEXT NOT NULL REFERENCES amazon_orders(order_id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    asin TEXT,
    price NUMERIC,
    quantity INTEGER NOT NULL DEFAULT 1,
    seller TEXT,
    image_url TEXT
);

CREATE INDEX idx_amazon_items_order_id ON amazon_items (order_id);
CREATE INDEX idx_amazon_items_asin ON amazon_items (asin);

CREATE TABLE amazon_matches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    amazon_transaction_id UUID NOT NULL REFERENCES amazon_transactions(id) ON DELETE CASCADE,
    bank_transaction_id UUID NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    confidence TEXT NOT NULL DEFAULT 'Exact',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (amazon_transaction_id, bank_transaction_id)
);

CREATE INDEX idx_amazon_matches_bank_txn ON amazon_matches (bank_transaction_id);
