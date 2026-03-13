use std::collections::HashMap;

use sqlx::Row;

use budget_core::models::{BudgetConfig, Category, CategoryId};

use crate::{Db, DbError, row_to_category};

impl Db {
    /// Insert a new category.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails (e.g. duplicate primary key).
    pub async fn insert_category(&self, category: &Category) -> Result<(), DbError> {
        let pool = &self.0;
        let (start, end) = match &category.budget {
            BudgetConfig::Project {
                start_date,
                end_date,
                ..
            } => (Some(*start_date), *end_date),
            _ => (None, None),
        };
        sqlx::query(
            "INSERT INTO categories (id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(category.id)
        .bind(&category.name)
        .bind(category.parent_id)
        .bind(category.budget.mode().map(|m| m.to_string()))
        .bind(category.budget.budget_type().map(|t| t.to_string()))
        .bind(category.budget.amount())
        .bind(start)
        .bind(end)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update all mutable fields of an existing category.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn update_category(&self, category: &Category) -> Result<(), DbError> {
        let pool = &self.0;
        let (start, end) = match &category.budget {
            BudgetConfig::Project {
                start_date,
                end_date,
                ..
            } => (Some(*start_date), *end_date),
            _ => (None, None),
        };
        sqlx::query(
            "UPDATE categories SET name = $1, parent_id = $2, budget_mode = $3, budget_type = $4,
                    budget_amount = $5, project_start_date = $6, project_end_date = $7
             WHERE id = $8",
        )
        .bind(&category.name)
        .bind(category.parent_id)
        .bind(category.budget.mode().map(|m| m.to_string()))
        .bind(category.budget.budget_type().map(|t| t.to_string()))
        .bind(category.budget.amount())
        .bind(start)
        .bind(end)
        .bind(category.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all categories.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_categories(&self) -> Result<Vec<Category>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date FROM categories ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        rows.iter().map(row_to_category).collect()
    }

    /// Count transactions per category.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn category_transaction_counts(&self) -> Result<HashMap<CategoryId, i64>, DbError> {
        let pool = &self.0;
        let rows = sqlx::query(
            "SELECT category_id, COUNT(*) as cnt FROM transactions WHERE category_id IS NOT NULL GROUP BY category_id",
        )
        .fetch_all(pool)
        .await?;
        let mut map = HashMap::new();
        for row in &rows {
            let id: CategoryId = row.try_get("category_id")?;
            let count: i64 = row.try_get("cnt")?;
            map.insert(id, count);
        }
        Ok(map)
    }

    /// Get a single category by its ID.
    ///
    /// Returns `None` if no category with the given ID exists.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn get_category(&self, id: CategoryId) -> Result<Option<Category>, DbError> {
        let pool = &self.0;
        let row = sqlx::query(
            "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date
             FROM categories WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(row_to_category).transpose()
    }

    /// Check whether a category has any child categories.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn category_has_children(&self, id: CategoryId) -> Result<bool, DbError> {
        let pool = &self.0;
        let row =
            sqlx::query("SELECT EXISTS(SELECT 1 FROM categories WHERE parent_id = $1) as has")
                .bind(id)
                .fetch_one(pool)
                .await?;
        Ok(row.try_get::<bool, _>("has")?)
    }

    /// Find a category by name, supporting both storage conventions.
    ///
    /// Resolution order:
    /// 1. Exact match on `name` (handles legacy categories stored as `"Food:Groceries"`).
    /// 2. If `name` contains a colon (e.g. `"Food:Groceries"`), look for a child
    ///    named `"Groceries"` whose parent is named `"Food"`.
    ///
    /// Returns `None` if no match is found.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if a database query fails.
    pub async fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, DbError> {
        let pool = &self.0;

        // 1. Exact match on stored name
        let row = sqlx::query(
            "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date
             FROM categories WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some(r) = row.as_ref() {
            return row_to_category(r).map(Some);
        }

        // 2. Colon-qualified lookup: "Parent:Child" -> find child under parent
        if let Some((parent_name, child_name)) = budget_core::models::parse_qualified_name(name) {
            let row = sqlx::query(
                "SELECT c.id, c.name, c.parent_id, c.budget_mode, c.budget_type, c.budget_amount, c.project_start_date, c.project_end_date
                 FROM categories c
                 JOIN categories p ON c.parent_id = p.id
                 WHERE p.name = $1 AND c.name = $2",
            )
            .bind(parent_name)
            .bind(child_name)
            .fetch_optional(pool)
            .await?;

            if let Some(r) = row.as_ref() {
                return row_to_category(r).map(Some);
            }

            // 3. Fuzzy fallback: match just the child name (case-insensitive)
            //    against any leaf category, ignoring the parent the LLM guessed.
            //    e.g. "Housing:Utilities" resolves to "Utilities" under "House".
            let row = sqlx::query(
                "SELECT id, name, parent_id, budget_mode, budget_type, budget_amount, project_start_date, project_end_date
                 FROM categories WHERE LOWER(name) = LOWER($1)",
            )
            .bind(child_name)
            .fetch_optional(pool)
            .await?;

            if let Some(r) = row.as_ref() {
                return row_to_category(r).map(Some);
            }
        }

        Ok(None)
    }

    /// List all category names in fully-qualified `"Parent:Child"` format.
    ///
    /// Used to pass existing categories to the LLM so it maps to known names.
    /// Handles both naming conventions: categories whose `name` already
    /// contains the parent prefix and those that store only the leaf name
    /// with a `parent_id` reference.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if the query fails.
    pub async fn list_category_names(&self) -> Result<Vec<String>, DbError> {
        let categories = self.list_categories().await?;
        let qualified = budget_core::models::build_qualified_name_map(&categories);
        let mut names: Vec<String> = qualified.into_values().collect();
        names.sort();
        Ok(names)
    }

    /// Delete a category by its ID, clearing all foreign-key references first.
    ///
    /// Nullifies `category_id` on transactions, `target_category_id` on rules,
    /// and `parent_id` on child categories before removing the row.
    ///
    /// # Errors
    ///
    /// Returns `DbError` if any query fails.
    pub async fn delete_category(&self, id: CategoryId) -> Result<(), DbError> {
        let pool = &self.0;
        let mut tx = pool.begin().await?;

        sqlx::query("UPDATE transactions SET category_id = NULL WHERE category_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE rules SET target_category_id = NULL WHERE target_category_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE categories SET parent_id = NULL WHERE parent_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }
}
