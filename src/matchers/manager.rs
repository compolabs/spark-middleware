use std::collections::HashMap;

pub struct MatcherManager {
    pub matchers: HashMap<String, usize>,  
}

impl MatcherManager {
    pub fn new() -> Self {
        Self {
            matchers: HashMap::new(),
        }
    }

    
    pub fn add_matcher(&mut self, uuid: String) {
        self.matchers.insert(uuid, 0);
    }

    
    pub fn remove_matcher(&mut self, uuid: &String) {
        self.matchers.remove(uuid);
    }

    
    pub fn select_matcher(&mut self) -> Option<String> {
        self.matchers
            .iter_mut()
            .min_by_key(|entry| *entry.1) 
            .map(|(uuid, _)| uuid.clone())
    }

    
    pub fn increase_load(&mut self, uuid: &String, load: usize) {
        if let Some(count) = self.matchers.get_mut(uuid) {
            *count += load;
        }
    }

    
    pub fn decrease_load(&mut self, uuid: &String, load: usize) {
        if let Some(count) = self.matchers.get_mut(uuid) {
            if *count >= load {
                *count -= load;
            }
        }
    }

    
    pub fn get_load(&self, uuid: &String) -> usize {
        *self.matchers.get(uuid).unwrap_or(&0)
    }

    
    pub fn reset_load(&mut self) {
        for (_, load) in self.matchers.iter_mut() {
            *load = 0;
        }
    }
}

impl Default for MatcherManager {
    fn default() -> Self {
        Self::new()
    }
}
