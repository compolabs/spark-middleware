use std::collections::HashMap;
use std::sync::Arc;

use rocket::{Build, Rocket, State};
use rocket_okapi::swagger_ui::make_swagger_ui;
use crate::middleware::aggregator::Aggregator;
use crate::middleware::manager::OrderManager;
use crate::web::routes::{get_routes, get_docs};

pub fn rocket(
    order_managers: HashMap<String, Arc<OrderManager>>,  
    aggregator: Arc<Aggregator>,  
) -> Rocket<Build> {
    rocket::build()
        .manage(order_managers)  
        .manage(aggregator)
        .mount("/", get_routes())  
        .mount("/swagger", make_swagger_ui(&get_docs()))  
}
