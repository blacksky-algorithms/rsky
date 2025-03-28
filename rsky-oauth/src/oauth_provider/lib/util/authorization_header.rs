pub struct AuthorizationHeader(String);

impl AuthorizationHeader {
    pub fn new(header: impl Into<String>) -> Result<Self, AuthorizationHeaderError> {
        let header = header.into();
        if header.is_empty() {
            return Err(AuthorizationHeaderError::Empty);
        }
        Ok(Self(header))
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthorizationHeaderError {
    #[error("Refresh token cannot be empty")]
    Empty,
}
