-- A source's default due date for items without one: due_days after the
-- time the source reports for each item, counting only weekdays if
-- due_business_days. NULL due_days means no default.
ALTER TABLE sources ADD COLUMN due_days INTEGER;
ALTER TABLE sources ADD COLUMN due_business_days BOOLEAN NOT NULL DEFAULT FALSE;
