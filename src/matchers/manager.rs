use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MatcherManager {
    pub matchers: HashMap<String, MatcherInfo>,
}

#[derive(Debug, Clone, Copy)]
pub struct MatcherInfo {
    pub load: usize,  
    pub active_batches: usize, 
}


impl MatcherManager {
    pub fn new() -> Self {
        Self {
            matchers: HashMap::new(),
        }
    }

    pub fn add_matcher(&mut self, uuid: String) {
        self.matchers.insert(uuid, MatcherInfo { load: 0, active_batches: 0 });
    }

    pub fn remove_matcher(&mut self, uuid: &String) {
        self.matchers.remove(uuid);
    }

    pub fn select_matcher(&mut self) -> Option<String> {
        self.matchers
            .iter_mut()
            .min_by_key(|entry| entry.1.active_batches)
            .map(|(uuid, _)| uuid.clone())
    }

    pub fn increase_load(&mut self, uuid: &String, load: usize) {
        if let Some(info) = self.matchers.get_mut(uuid) {
            info.load += load;
        }
    }

    pub fn increase_active_batches(&mut self, uuid: &String) {
        if let Some(info) = self.matchers.get_mut(uuid) {
            info.active_batches += 1;
        }
    }

    pub fn decrease_active_batches(&mut self, uuid: &String) {
        if let Some(info) = self.matchers.get_mut(uuid) {
            if info.active_batches > 0 {
                info.active_batches -= 1;
            }
        }
    }

    pub fn get_active_batches(&self, uuid: &String) -> usize {
        self.matchers.get(uuid).map_or(0, |info| info.active_batches)
    }

    pub fn reset_load(&mut self) {
        for (_, info) in self.matchers.iter_mut() {
            info.load = 0;
            info.active_batches = 0;
        }
    }
}
