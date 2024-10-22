use crate::indexer::spot_order::OrderType;
use crate::storage::order_book::OrderBook;
use async_graphql::{Context, Object, SimpleObject};
use std::sync::Arc;

#[derive(SimpleObject, Clone)]
struct Order {
    id: String,
    user: String,
    asset: String,
    amount: String,
    price: String,
    timestamp: u64,
    order_type: String,
    status: Option<String>,
}

pub struct Query;

#[Object]
impl Query {
    pub async fn buy_orders(&self, ctx: &Context<'_>) -> Vec<Order> {
        let order_book = ctx.data::<Arc<OrderBook>>().unwrap();
        let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
        buy_orders
            .into_iter()
            .map(|order| Order {
                id: order.id,
                user: order.user,
                asset: order.asset,
                amount: order.amount.to_string(),
                price: order.price.to_string(),
                timestamp: order.timestamp,
                order_type: "Buy".to_string(),
                status: order.status.map(|s| format!("{:?}", s)),
            })
            .collect()
    }

    pub async fn sell_orders(&self, ctx: &Context<'_>) -> Vec<Order> {
        let order_book = ctx.data::<Arc<OrderBook>>().unwrap();
        let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);
        sell_orders
            .into_iter()
            .map(|order| Order {
                id: order.id,
                user: order.user,
                asset: order.asset,
                amount: order.amount.to_string(),
                price: order.price.to_string(),
                timestamp: order.timestamp,
                order_type: "Sell".to_string(),
                status: order.status.map(|s| format!("{:?}", s)),
            })
            .collect()
    }

    pub async fn spread(&self, ctx: &Context<'_>) -> Option<String> {
        let order_book = ctx.data::<Arc<OrderBook>>().unwrap();
        let buy_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Buy);
        let sell_orders = order_book.get_orders_in_range(0, u128::MAX, OrderType::Sell);

        let max_buy_price = buy_orders.iter().map(|o| o.price).max();
        let min_sell_price = sell_orders.iter().map(|o| o.price).min();

        if let (Some(max_buy), Some(min_sell)) = (max_buy_price, min_sell_price) {
            Some((min_sell as i128 - max_buy as i128).to_string())
        } else {
            None
        }
    }
}
