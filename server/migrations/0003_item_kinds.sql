-- What kind of thing each item is: task, issue or patch. Existing items are
-- replaced with their real kind on their source's next fetch.
ALTER TABLE items ADD COLUMN kind TEXT NOT NULL DEFAULT 'task';

-- The item kinds each context hides, as a JSON array.
ALTER TABLE contexts ADD COLUMN hidden_kinds TEXT NOT NULL DEFAULT '[]';
