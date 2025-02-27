use crate::storage::{matching_orders::MatchingOrders, order_book::OrderBook};
use std::sync::Arc;

#[derive(Default)]
pub struct OrderStorage {
    pub order_book: Arc<OrderBook>,
    pub matching_orders: Arc<MatchingOrders>,
}

impl OrderStorage {
    pub fn new() -> Self {
        Self {
            order_book: Arc::new(OrderBook::new()),
            matching_orders: Arc::new(MatchingOrders::new()),
        }
    }

    pub fn clone_order_book(&self) -> Arc<OrderBook> {
        Arc::clone(&self.order_book)
    }

    pub fn clone_matching_orders(&self) -> Arc<MatchingOrders> {
        Arc::clone(&self.matching_orders)
    }
}
