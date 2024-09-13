use async_graphql::{Schema, Object, Context, SimpleObject, EmptyMutation, EmptySubscription};
use std::collections::HashMap;
use std::sync::Arc;
use crate::middleware::manager::OrderManager;
use crate::middleware::aggregator::Aggregator;
use crate::indexer::spot_order::OrderType;

#[derive(SimpleObject)]
struct Order {
    id: String,
    price: String, 
    amount: String, 
    order_type: String,
}

#[derive(Default)]
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn orders_by_indexer(
        &self,
        ctx: &Context<'_>,
        indexer: String,
    ) -> Vec<Order> {
        let managers = ctx.data::<HashMap<String, Arc<OrderManager>>>().unwrap();
        if let Some(manager) = managers.get(&indexer) {
            let buy_orders = manager.get_all_buy_orders().await;
            let sell_orders = manager.get_all_sell_orders().await;

            buy_orders
                .into_iter()
                .chain(sell_orders)
                .map(|o| Order {
                    id: o.id,
                    price: o.price.to_string(),  
                    amount: o.amount.to_string(),
                    order_type: format!("{:?}", o.order_type),
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    async fn aggregated_orders(&self, ctx: &Context<'_>) -> Vec<Order> {
        let aggregator = ctx.data::<Arc<Aggregator>>().unwrap();
        let buy_orders = aggregator.get_aggregated_orders(OrderType::Buy).await;
        let sell_orders = aggregator.get_aggregated_orders(OrderType::Sell).await;

        buy_orders
            .into_iter()
            .chain(sell_orders)
            .map(|o| Order {
                id: o.id,
                price: o.price.to_string(),  
                amount: o.amount.to_string(),
                order_type: format!("{:?}", o.order_type),
            })
            .collect()
    }
}

pub type AppSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub fn create_schema() -> AppSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish()
}
