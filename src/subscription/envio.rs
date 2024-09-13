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

pub fn format_graphql_subscription_old(order_type: OrderType) -> String {
    let limit = ev("FETCH_ORDER_LIMIT").unwrap_or_default();
    let (order_type_str, order_by) = match order_type {
        OrderType::Sell => ("Sell", "asc"),
        OrderType::Buy => ("Buy", "desc"),
    };

    format!(
        r#"subscription {{
            Order(
                limit: {}, 
                where: {{ status: {{_eq: "Active"}}, order_type: {{_eq: "{}"}} }}, 
                order_by: {{price: {}}}
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
        limit, order_type_str, order_by
    )
}
