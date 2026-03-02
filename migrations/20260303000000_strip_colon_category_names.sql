-- Strip redundant parent prefix from category names that store the full
-- qualified "Parent:Child" form while also having parent_id set.
-- After this migration, names store only the leaf portion; hierarchy is
-- expressed entirely through parent_id.

UPDATE categories
SET name = SUBSTRING(name FROM POSITION(':' IN name) + 1)
WHERE parent_id IS NOT NULL
  AND name LIKE '%:%';
