// @generated automatically by Diesel CLI.

diesel::table! {
    order_lines (id) {
        id -> Uuid,
        order_id -> Uuid,
        product_id -> Uuid,
        quantity -> Int4,
        unit_price -> Numeric,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    orders (id) {
        id -> Uuid,
        customer_id -> Uuid,
        #[max_length = 50]
        status -> Varchar,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    commerce_order_outbox (id) {
        id -> Uuid,
        #[max_length = 255]
        aggregate_type -> Varchar,
        #[max_length = 255]
        aggregate_id -> Varchar,
        #[max_length = 255]
        event_type -> Varchar,
        payload -> Jsonb,
        created_at -> Timestamptz,
    }
}

diesel::joinable!(order_lines -> orders (order_id));

diesel::allow_tables_to_appear_in_same_query!(order_lines, orders, commerce_order_outbox,);
