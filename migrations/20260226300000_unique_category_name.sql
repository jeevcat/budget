CREATE UNIQUE INDEX idx_categories_unique_name_parent
    ON categories (name, COALESCE(parent_id, ''));
