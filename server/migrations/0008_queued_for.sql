-- The start of the (local) day an item is queued for. From then on it's shown
-- under Today until the queue is cleared.
ALTER TABLE item_state ADD COLUMN queued_for INTEGER;
