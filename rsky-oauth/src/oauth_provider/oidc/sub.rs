use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Sub(String);

impl Sub {
    pub fn new(id: String) -> Result<Self, SubError> {
        if id.len() < 1 {
            Err(SubError::TypeError("Sub cannot be empty".to_string()))
        } else {
            Ok(Sub(id))
        }
    }

    pub fn get(&self) -> String {
        self.0.clone()
    }
}

pub enum SubError {
    TypeError(String),
}
