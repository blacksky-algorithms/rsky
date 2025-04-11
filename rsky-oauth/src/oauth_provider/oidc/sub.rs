use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub struct Sub(String);

impl Sub {
    pub fn new(id: impl Into<String>) -> Result<Self, SubError> {
        let id = id.into();
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

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub enum SubError {
    TypeError(String),
}
