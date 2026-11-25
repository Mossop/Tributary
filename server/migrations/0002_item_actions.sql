-- Quick actions each item offers, as a JSON array of
-- {"id", "label", "icon"} objects.
ALTER TABLE items ADD COLUMN actions TEXT NOT NULL DEFAULT '[]';
