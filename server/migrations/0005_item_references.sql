-- Identifiers of the things each item is about or refers to, as a JSON array
-- of strings (e.g. "bugzilla:123", "phab:D456"). Items sharing one are shown
-- together.
ALTER TABLE items ADD COLUMN refs TEXT NOT NULL DEFAULT '[]';
