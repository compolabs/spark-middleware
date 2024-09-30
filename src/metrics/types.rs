use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use log::{info, error};

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct OrderMetrics {
    pub total_matched: u64,
    pub total_remaining: u64,
    pub matched_per_second: f64,
    pub last_update: Option<DateTime<Utc>>, 
    pub match_count: u64,
}

impl OrderMetrics {
    pub fn new() -> Self {
        OrderMetrics {
            total_matched: 0,
            total_remaining: 0,
            matched_per_second: 0.0,
            last_update: None,
            match_count: 0,
        }
    }

    pub fn update_metrics(&mut self, matched_count: u64, remaining_count: u64) {
        self.total_matched += matched_count;
        self.total_remaining = remaining_count;
        self.match_count += matched_count;

        if let Some(last) = self.last_update {
            let duration = Utc::now().signed_duration_since(last).to_std().unwrap_or(Duration::new(0, 0));
            if duration.as_secs() > 0 {
                self.matched_per_second = self.match_count as f64 / duration.as_secs_f64();
                self.match_count = 0;
                self.last_update = Some(Utc::now());
            }
        } else {
            self.last_update = Some(Utc::now());
        }

        self.save_to_file();
    }

    pub fn save_to_file(&self) {
        match serde_json::to_string(&self) {
            Ok(json) => {
                if let Err(e) = fs::write("order_metrics.json", json) {
                    error!("Failed to save metrics: {}", e);
                }
            }
            Err(e) => error!("Failed to serialize metrics: {}", e),
        }
    }

    pub fn load_from_file() -> Self {
        match fs::read_to_string("order_metrics.json") {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(metrics) => metrics,
                Err(e) => {
                    error!("Failed to deserialize metrics: {}", e);
                    Self::new()
                }
            },
            Err(e) => {
                error!("Failed to read metrics file: {}", e);
                Self::new()
            }
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
        self.save_to_file();
    }
}
