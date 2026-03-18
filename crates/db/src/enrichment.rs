use std::collections::HashMap;

use super::{Db, DbError};

impl Db {
    /// Get enrichment item titles for a batch of bank transactions.
    ///
    /// Queries all enrichment match tables (Amazon, `PayPal`) and merges
    /// results into a single map.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if any query fails.
    pub async fn get_enrichment_item_titles_for_transactions(
        &self,
        bank_txn_ids: &[uuid::Uuid],
    ) -> Result<HashMap<uuid::Uuid, Vec<String>>, DbError> {
        let mut map = self
            .get_amazon_item_titles_for_transactions(bank_txn_ids)
            .await?;

        let paypal_map = self
            .get_paypal_item_titles_for_transactions(bank_txn_ids)
            .await?;
        for (txn_id, titles) in paypal_map {
            map.entry(txn_id).or_default().extend(titles);
        }

        // Deduplicate titles per transaction (in case multiple sources match)
        for titles in map.values_mut() {
            titles.sort();
            titles.dedup();
        }

        Ok(map)
    }
}
