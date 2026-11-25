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

-- `id` is the slug used in URLs. It can be changed, cascading to
-- source_contexts.
CREATE TABLE contexts (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    icon TEXT,
    position INTEGER NOT NULL
);

-- Settings common to every source type are columns; anything specific to the
-- source type is in `config` as a JSON object.
CREATE TABLE sources (
    id TEXT PRIMARY KEY NOT NULL,
    type TEXT NOT NULL,
    name TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE source_contexts (
    source_id TEXT NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    context_id TEXT NOT NULL REFERENCES contexts (id) ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (source_id, context_id)
);
