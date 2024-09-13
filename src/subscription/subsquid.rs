use crate::{config::env::ev, indexer::spot_order::OrderType};

pub fn format_graphql_subscription(order_type: OrderType) -> String {
    let limit = ev("FETCH_ORDER_LIMIT").unwrap_or("10".to_string());
    let order_type_str = match order_type {
        OrderType::Sell => "activeSellOrders",
        OrderType::Buy => "activeBuyOrders",
    };

    format!(
        r#"
        subscription {{
            {}(limit: {}) {{
                id
                asset
                amount
                orderType
                price
                user
                status
                initialAmount
                timestamp
            }}
        }}
        "#,
        order_type_str, limit
    )
}
