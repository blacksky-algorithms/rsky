use crate::oauth_types::OAuthRedirectUri;

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
            unimplemented!()
        }
        _ => false,
    }
}
