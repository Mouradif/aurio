use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum LuaValue {
    Number(f64),
    Boolean(bool),
    String(String),
    Nil,
}

pub struct VariableStore {
    node_vars: HashMap<(usize, String, String), LuaValue>, // (track_id, node_id, var_name)
    track_vars: HashMap<(usize, String), LuaValue>,        // (track_id, var_name)
    global_vars: HashMap<String, LuaValue>,                // var_name
}

impl VariableStore {
    pub fn new() -> Self {
        Self {
            node_vars: HashMap::new(),
            track_vars: HashMap::new(),
            global_vars: HashMap::new(),
        }
    }

    pub fn get_node_var(&self, track_id: usize, node_id: &str, name: &str) -> LuaValue {
        self.node_vars
            .get(&(track_id, node_id.to_string(), name.to_string()))
            .cloned()
            .unwrap_or(LuaValue::Nil)
    }

    pub fn set_node_var(&mut self, track_id: usize, node_id: &str, name: &str, value: LuaValue) {
        self.node_vars
            .insert((track_id, node_id.to_string(), name.to_string()), value);
    }

    pub fn get_track_var(&self, track_id: usize, name: &str) -> LuaValue {
        self.track_vars
            .get(&(track_id, name.to_string()))
            .cloned()
            .unwrap_or(LuaValue::Nil)
    }

    pub fn set_track_var(&mut self, track_id: usize, name: &str, value: LuaValue) {
        self.track_vars.insert((track_id, name.to_string()), value);
    }

    pub fn get_global(&self, name: &str) -> LuaValue {
        self.global_vars.get(name).cloned().unwrap_or(LuaValue::Nil)
    }

    pub fn set_global(&mut self, name: &str, value: LuaValue) {
        self.global_vars.insert(name.to_string(), value);
    }
}
