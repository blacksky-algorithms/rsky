use crate::oauth_types::OAuthRedirectUri;
use url::Url;

/**
 *
 * @see {@link https://datatracker.ietf.org/doc/html/rfc8252#section-8.4}
 * @see {@link https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1-11#section-8.4.2}
 */
pub fn compare_redirect_uri(allowed_uri: OAuthRedirectUri, request_uri: OAuthRedirectUri) -> bool {
    // https://datatracker.ietf.org/doc/html/rfc8252#section-8.4
    //
    // > Authorization servers MUST require clients to register their complete
    // > redirect URI (including the path component) and reject authorization
    // > requests that specify a redirect URI that doesn't exactly match the
    // > one that was registered; the exception is loopback redirects, where
    // > an exact match is required except for the port URI component.
    if allowed_uri == request_uri {
        return true;
    }

    // https://datatracker.ietf.org/doc/html/rfc8252#section-7.3
    match allowed_uri {
        OAuthRedirectUri::Loopback(allowed_uri) => {
            // > The authorization server MUST allow any port to be specified at the
            // > time of the request for loopback IP redirect URIs, to accommodate
            // > clients that obtain an available ephemeral port from the operating
            // > system at the time of the request
            //
            // Note: We only apply this rule if the allowed URI does not have a port
            // specified.
            let allowed_url = Url::parse(allowed_uri.as_str()).unwrap();
            let request_url = Url::parse(request_uri.as_str()).unwrap();

            let port_match = if let Some(allowed_port) = allowed_url.port() {
                if let Some(request_port) = request_url.port() {
                    request_port == allowed_port
                } else {
                    false
                }
            } else {
                true
            };

            port_match
                && allowed_url.host() == request_url.host()
                && allowed_url.path() == request_url.path()
                && allowed_url.query() == request_url.query()
                && allowed_url.username() == request_url.username()
                && allowed_url.password() == request_url.password()
                && allowed_url.scheme() == request_url.scheme()
                && allowed_url.fragment() == request_url.fragment()
        }
        _ => false,
    }
}
