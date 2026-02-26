-- Replace per-rule match_field/match_pattern with a JSONB conditions array.
-- Each element is {"field": "<match_field>", "pattern": "<match_pattern>"}.
-- All conditions must match for the rule to fire (AND semantics).

ALTER TABLE rules ADD COLUMN conditions JSONB;

UPDATE rules SET conditions = jsonb_build_array(
    jsonb_build_object('field', match_field, 'pattern', match_pattern)
);

ALTER TABLE rules ALTER COLUMN conditions SET NOT NULL;

ALTER TABLE rules DROP COLUMN match_field;
ALTER TABLE rules DROP COLUMN match_pattern;
