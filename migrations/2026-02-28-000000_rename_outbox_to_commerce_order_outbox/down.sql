DROP INDEX IF EXISTS commerce_order_outbox_aggregate_id_idx;
DROP INDEX IF EXISTS commerce_order_outbox_created_at_idx;
ALTER TABLE commerce_order_outbox RENAME TO outbox;
