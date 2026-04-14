use std::fmt;

#[derive(Debug)]
pub enum GraphError {
    Other(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GraphError {}
