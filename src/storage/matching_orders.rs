use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Default)]
pub struct MatchingOrders {
    orders: Mutex<HashSet<String>>,
}

impl MatchingOrders {
    pub fn new() -> Self {
        Self {
            orders: Mutex::new(HashSet::new()),
        }
    }

    pub fn add(&self, order_id: &str) {
        let mut orders = self.orders.lock().unwrap();
        orders.insert(order_id.to_string());
    }

    pub fn remove(&self, order_id: &str) {
        let mut orders = self.orders.lock().unwrap();
        orders.remove(order_id);
    }

    pub fn contains(&self, order_id: &str) -> bool {
        let orders = self.orders.lock().unwrap();
        orders.contains(order_id)
    }

    pub fn get_all(&self) -> HashSet<String> {
        let orders = self.orders.lock().unwrap();
        orders.clone()
    }
}
