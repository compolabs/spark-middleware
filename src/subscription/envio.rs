use crate::{config::env::ev, indexer::spot_order::OrderType};

pub fn format_graphql_subscription(order_type: OrderType) -> String {
    let limit = ev("FETCH_ORDER_LIMIT").unwrap_or_default();
    let order_type_str = match order_type {
        OrderType::Sell => "ActiveSellOrder",
        OrderType::Buy => "ActiveBuyOrder",
    };

    format!(
        r#"subscription {{
            {}(
                limit: {}
            ) {{
                id
                user
                timestamp
                order_type
                amount
                asset
                price
            }}
        }}"#,
        order_type_str, limit
    )
}
