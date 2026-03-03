-- Strip redundant parent prefix from category names that store the full
-- qualified "Parent:Child" form while also having parent_id set.
-- After this migration, names store only the leaf portion; hierarchy is
-- expressed entirely through parent_id.
--
-- Uses reverse + split to strip everything up to the last colon, handling
-- multi-level names like "A:B:C" → "C".

UPDATE categories
SET name = REVERSE(SPLIT_PART(REVERSE(name), ':', 1))
WHERE parent_id IS NOT NULL
  AND name LIKE '%:%';
