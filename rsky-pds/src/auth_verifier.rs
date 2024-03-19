use anyhow::{Result, bail};

pub enum AuthScope {
    Access,
    Refresh,
    AppPass,
    Deactivated,
}

impl AuthScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthScope::Access => "com.atproto.access",
            AuthScope::Refresh => "com.atproto.refresh",
            AuthScope::AppPass => "com.atproto.appPass",
            AuthScope::Deactivated => "com.atproto.deactivated",
        }
    }
    
    pub fn from_str(scope: &str) -> Result<Self> {
        match scope {
            "com.atproto.access" => Ok(AuthScope::Access),
            "com.atproto.refresh" => Ok(AuthScope::Refresh),
            "com.atproto.appPass" => Ok(AuthScope::AppPass),
            "com.atproto.deactivated" => Ok(AuthScope::Deactivated),
            _ => bail!("Invalid AuthScope: `{scope:?}` is not a valid auth scope")
        }
    }
}
