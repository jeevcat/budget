use budget_core::models::{Rule, RuleId, RuleType};

use crate::{Db, DbError, row_to_rule};

impl Db {
    /// Insert a new rule.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn insert_rule(&self, rule: &Rule) -> Result<(), DbError> {
        let pool = &self.0;
        let conditions_json = serde_json::to_string(&rule.conditions)
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        sqlx::query(
            "INSERT INTO rules (id, rule_type, conditions, target_category_id, target_correlation_type, priority)
             VALUES ($1, $2, $3::jsonb, $4, $5, $6)",
        )
        .bind(rule.id)
        .bind(rule.rule_type.to_string())
        .bind(&conditions_json)
        .bind(rule.target_category_id)
        .bind(rule.target_correlation_type.map(|ct| ct.to_string()))
        .bind(rule.priority)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all rules.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_rules(&self) -> Result<Vec<Rule>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, rule_type, conditions::text as conditions, target_category_id, target_correlation_type, priority
             FROM rules",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_rule).collect()
    }

    /// List rules filtered by type, ordered by priority descending.
    ///
    /// Higher-priority rules are returned first.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_rules_by_type(&self, rule_type: RuleType) -> Result<Vec<Rule>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, rule_type, conditions::text as conditions, target_category_id, target_correlation_type, priority
             FROM rules
             WHERE rule_type = $1
             ORDER BY priority DESC",
        )
        .bind(rule_type.to_string())
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_rule).collect()
    }

    /// Get a single rule by its ID.
    ///
    /// Returns `None` if no rule with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_rule(&self, id: RuleId) -> Result<Option<Rule>, DbError> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, rule_type, conditions::text as conditions, target_category_id, target_correlation_type, priority
             FROM rules WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_rule).transpose()
    }

    /// Update all mutable fields of an existing rule.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn update_rule(&self, rule: &Rule) -> Result<(), DbError> {
        let pool = &self.0;
        let conditions_json = serde_json::to_string(&rule.conditions)
            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
        sqlx::query(
            "UPDATE rules SET rule_type = $1, conditions = $2::jsonb,
                    target_category_id = $3, target_correlation_type = $4, priority = $5
             WHERE id = $6",
        )
        .bind(rule.rule_type.to_string())
        .bind(&conditions_json)
        .bind(rule.target_category_id)
        .bind(rule.target_correlation_type.map(|ct| ct.to_string()))
        .bind(rule.priority)
        .bind(rule.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a rule by its ID.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn delete_rule(&self, id: RuleId) -> Result<(), DbError> {
        let pool = &self.0;
        sqlx::query("DELETE FROM rules WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
