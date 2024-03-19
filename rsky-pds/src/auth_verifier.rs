
pub enum AuthScope {
    Access,
    Refresh,
    AppPass,
    Deactivated
}

impl AuthScope {
    fn as_str(&self) -> &'static str {
        match self {
            AuthScope::Access => "com.atproto.access",
            AuthScope::Refresh => "com.atproto.refresh",
            AuthScope::AppPass => "com.atproto.appPass",
            AuthScope::Deactivated => "com.atproto.deactivated"
        }
    }
}
