-- Which of an item's times a source's default due date counts from:
-- 'requested' (the source specific time), 'created' or 'modified'. Existing
-- default due dates all counted from the requested time.
ALTER TABLE sources ADD COLUMN due_from TEXT;
UPDATE sources SET due_from = 'requested' WHERE due_days IS NOT NULL;
