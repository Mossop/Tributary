-- Timestamps are stored as milliseconds since the Unix epoch (UTC).

-- The most recently fetched items for each source. Replaced wholesale on
-- each successful fetch.
CREATE TABLE items (
    source_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    icon TEXT,
    description TEXT,
    modified INTEGER NOT NULL,
    due INTEGER,
    PRIMARY KEY (source_id, item_id)
);

-- User state for items. Kept separate from items so that refetching does not
-- lose it.
CREATE TABLE item_state (
    source_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    snoozed_until INTEGER,
    snoozed_forever INTEGER NOT NULL DEFAULT 0,
    due_override INTEGER,
    PRIMARY KEY (source_id, item_id)
);

CREATE TABLE source_status (
    source_id TEXT PRIMARY KEY NOT NULL,
    last_attempt INTEGER,
    last_success INTEGER,
    last_error TEXT
);
