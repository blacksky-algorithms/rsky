use serde::de::Visitor;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Sub(String);

impl Sub {
    pub fn new(id: impl Into<String>) -> Result<Self, SubError> {
        let id = id.into();
        if id.is_empty() {
            Err(SubError::TypeError("Sub cannot be empty".to_string()))
        } else {
            Ok(Sub(id))
        }
    }

    pub fn get(&self) -> String {
        self.0.clone()
    }
}

impl Serialize for Sub {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

// Custom visitor for deserialization
struct SubVisitor;

impl Visitor<'_> for SubVisitor {
    type Value = Sub;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string representing a Sub")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match Sub::new(value) {
            Ok(uri) => Ok(uri),
            Err(error) => Err(E::custom(format!("{:?}", error))),
        }
    }
}

// Implement Deserialize using the visitor
impl<'de> Deserialize<'de> for Sub {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(SubVisitor)
    }
}

impl Display for Sub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub enum SubError {
    TypeError(String),
}
