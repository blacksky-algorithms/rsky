use std::fmt;
use std::str::FromStr;

/// The type of entity in OpenID Connect claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OidcEntityType {
    /// Claims in ID Token
    IdToken,
    /// Claims in UserInfo endpoint response
    UserInfo,
}

impl OidcEntityType {
    /// Get a slice of all possible entity types
    pub fn variants() -> &'static [OidcEntityType] {
        &[
            OidcEntityType::IdToken,
            OidcEntityType::UserInfo,
        ]
    }
}

impl fmt::Display for OidcEntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OidcEntityType::IdToken => write!(f, "id_token"),
            OidcEntityType::UserInfo => write!(f, "userinfo"),
        }
    }
}

/// Error returned when parsing a string into an OidcEntityType fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Invalid OIDC entity type: {0}")]
pub struct ParseEntityTypeError(String);

impl FromStr for OidcEntityType {
    type Err = ParseEntityTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "id_token" => Ok(OidcEntityType::IdToken),
            "userinfo" => Ok(OidcEntityType::UserInfo),
            _ => Err(ParseEntityTypeError(s.to_string())),
        }
    }
}

impl AsRef<str> for OidcEntityType {
    fn as_ref(&self) -> &str {
        match self {
            OidcEntityType::IdToken => "id_token",
            OidcEntityType::UserInfo => "userinfo",
        }
    }
}

impl serde::Serialize for OidcEntityType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }
}

impl<'de> serde::Deserialize<'de> for OidcEntityType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use serde_json::json;

    #[test]
    fn test_variants() {
        let variants = OidcEntityType::variants();
        assert_eq!(variants.len(), 2);
        assert!(variants.contains(&OidcEntityType::IdToken));
        assert!(variants.contains(&OidcEntityType::UserInfo));
    }

    #[test]
    fn test_display() {
        assert_eq!(OidcEntityType::IdToken.to_string(), "id_token");
        assert_eq!(OidcEntityType::UserInfo.to_string(), "userinfo");
    }

    #[test]
    fn test_from_str() {
        assert_eq!("id_token".parse::<OidcEntityType>().unwrap(), OidcEntityType::IdToken);
        assert_eq!("userinfo".parse::<OidcEntityType>().unwrap(), OidcEntityType::UserInfo);
        
        assert!("invalid".parse::<OidcEntityType>().is_err());
    }

    #[test]
    fn test_as_ref() {
        assert_eq!(OidcEntityType::IdToken.as_ref(), "id_token");
        assert_eq!(OidcEntityType::UserInfo.as_ref(), "userinfo");
    }

    #[test]
    fn test_hash() {
        let mut set = HashSet::new();
        set.insert(OidcEntityType::IdToken);
        
        assert!(set.contains(&OidcEntityType::IdToken));
        assert!(!set.contains(&OidcEntityType::UserInfo));
    }

    #[test]
    fn test_serialize() {
        let serialized = serde_json::to_string(&OidcEntityType::IdToken).unwrap();
        assert_eq!(serialized, "\"id_token\"");
    }

    #[test]
    fn test_deserialize() {
        let deserialized: OidcEntityType = serde_json::from_str("\"id_token\"").unwrap();
        assert_eq!(deserialized, OidcEntityType::IdToken);
        
        let result: Result<OidcEntityType, _> = serde_json::from_str("\"invalid\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_in_structure() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct TestStruct {
            entity_type: OidcEntityType,
        }
        
        let test = TestStruct { entity_type: OidcEntityType::UserInfo };
        let json = json!({ "entity_type": "userinfo" });
        
        let serialized = serde_json::to_value(&test).unwrap();
        assert_eq!(serialized, json);
        
        let deserialized: TestStruct = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, test);
    }
}