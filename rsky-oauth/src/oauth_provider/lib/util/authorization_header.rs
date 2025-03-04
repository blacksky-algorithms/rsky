use crate::oauth_types::oauth_access_token::OAuthAccessToken;
use crate::oauth_types::oauth_token_type::OAuthTokenType;

pub fn parse_authorization_header(header: Option<String>) -> (OAuthTokenType, OAuthAccessToken) {
    match header {
        None => {
            panic!("Invalid Request")
        }
        Some(header) => {
            let res: Vec<&str> = header.split(" ").collect();
            (
                res.get(0).unwrap().to_string(),
                res.get(1).unwrap().to_string(),
            )
        }
    }
}
