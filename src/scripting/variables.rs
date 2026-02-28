use crate::graph::NodeId;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum LuaValue {
    Number(f64),
    Boolean(bool),
    String(String),
    Nil,
}

pub struct VariableStore {
    node_vars: HashMap<(NodeId, String), LuaValue>,
    global_vars: HashMap<String, LuaValue>,
}

impl Default for VariableStore {
    fn default() -> Self {
        Self::new()
    }
}

impl VariableStore {
    pub fn new() -> Self {
        Self {
            node_vars: HashMap::new(),
            global_vars: HashMap::new(),
        }
    }

    pub fn get_node_var(&self, node_id: &NodeId, name: &str) -> LuaValue {
        self.node_vars
            .get(&(node_id.clone(), name.to_string()))
            .cloned()
            .unwrap_or(LuaValue::Nil)
    }

    pub fn set_node_var(&mut self, node_id: &NodeId, name: &str, value: LuaValue) {
        self.node_vars
            .insert((node_id.clone(), name.to_string()), value);
    }

    pub fn get_global(&self, name: &str) -> LuaValue {
        self.global_vars.get(name).cloned().unwrap_or(LuaValue::Nil)
    }

    pub fn set_global(&mut self, name: &str, value: LuaValue) {
        self.global_vars.insert(name.to_string(), value);
    }
}
