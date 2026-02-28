ALTER TABLE outbox RENAME TO commerce_order_outbox;
CREATE INDEX ON commerce_order_outbox (aggregate_id);
CREATE INDEX ON commerce_order_outbox (created_at);
