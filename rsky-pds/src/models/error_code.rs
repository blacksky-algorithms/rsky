use std::fmt::Display;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "multiple_choices")]
    MultipleChoices,
    #[serde(rename = "moved_permanently")]
    MovedPermanently,
    #[serde(rename = "found")]
    Found,
    #[serde(rename = "see_other")]
    SeeOther,
    #[serde(rename = "not_modified")]
    NotModified,
    #[serde(rename = "use_proxy")]
    UseProxy,
    #[serde(rename = "temporary_redirect")]
    TemporaryRedirect,
    #[serde(rename = "permanent_redirect")]
    PermanentRedirect,
    #[serde(rename = "bad_request")]
    BadRequest,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "payment_required")]
    PaymentRequired,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "method_not_allowed")]
    MethodNotAllowed,
    #[serde(rename = "not_acceptable")]
    NotAcceptable,
    #[serde(rename = "proxy_authentication_required")]
    ProxyAuthenticationRequired,
    #[serde(rename = "request_timeout")]
    RequestTimeout,
    #[serde(rename = "conflict")]
    Conflict,
    #[serde(rename = "gone")]
    Gone,
    #[serde(rename = "length_required")]
    LengthRequired,
    #[serde(rename = "precondition_failed")]
    PreconditionFailed,
    #[serde(rename = "payload_too_large")]
    PayloadTooLarge,
    #[serde(rename = "uri_too_long")]
    UriTooLong,
    #[serde(rename = "unsupported_media_type")]
    UnsupportedMediaType,
    #[serde(rename = "range_not_satisfiable")]
    RangeNotSatisfiable,
    #[serde(rename = "expectation_failed")]
    ExpectationFailed,
    #[serde(rename = "im_a_teapot")]
    ImATeapot,
    #[serde(rename = "misdirected_request")]
    MisdirectedRequest,
    #[serde(rename = "unprocessable_entity")]
    UnprocessableEntity,
    #[serde(rename = "locked")]
    Locked,
    #[serde(rename = "failed_dependency")]
    FailedDependency,
    #[serde(rename = "upgrade_required")]
    UpgradeRequired,
    #[serde(rename = "precondition_required")]
    PreconditionRequired,
    #[serde(rename = "too_many_requests")]
    TooManyRequests,
    #[serde(rename = "request_header_fields_too_large")]
    RequestHeaderFieldsTooLarge,
    #[serde(rename = "unavailable_for_legal_reasons")]
    UnavailableForLegalReasons,
    #[serde(rename = "internal_server_error")]
    InternalServerError,
    #[serde(rename = "not_implemented")]
    NotImplemented,
    #[serde(rename = "bad_gateway")]
    BadGateway,
    #[serde(rename = "service_unavailable")]
    ServiceUnavailable,
    #[serde(rename = "gateway_timeout")]
    GatewayTimeout,
    #[serde(rename = "http_version_not_supported")]
    HttpVersionNotSupported,
    #[serde(rename = "variant_also_negotiates")]
    VariantAlsoNegotiates,
    #[serde(rename = "insufficient_storage")]
    InsufficientStorage,
    #[serde(rename = "loop_detected")]
    LoopDetected,
    #[serde(rename = "not_extended")]
    NotExtended,
    #[serde(rename = "network_authentication_required")]
    NetworkAuthenticationRequired,
}

impl Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Self::MultipleChoices => String::from("multiple_choices"),
            Self::MovedPermanently => String::from("moved_permanently"),
            Self::Found => String::from("found"),
            Self::SeeOther => String::from("see_other"),
            Self::NotModified => String::from("not_modified"),
            Self::UseProxy => String::from("use_proxy"),
            Self::TemporaryRedirect => String::from("temporary_redirect"),
            Self::PermanentRedirect => String::from("permanent_redirect"),
            Self::BadRequest => String::from("bad_request"),
            Self::Unauthorized => String::from("unauthorized"),
            Self::PaymentRequired => String::from("payment_required"),
            Self::Forbidden => String::from("forbidden"),
            Self::NotFound => String::from("not_found"),
            Self::MethodNotAllowed => String::from("method_not_allowed"),
            Self::NotAcceptable => String::from("not_acceptable"),
            Self::ProxyAuthenticationRequired => String::from("proxy_authentication_required"),
            Self::RequestTimeout => String::from("request_timeout"),
            Self::Conflict => String::from("conflict"),
            Self::Gone => String::from("gone"),
            Self::LengthRequired => String::from("length_required"),
            Self::PreconditionFailed => String::from("precondition_failed"),
            Self::PayloadTooLarge => String::from("payload_too_large"),
            Self::UriTooLong => String::from("uri_too_long"),
            Self::UnsupportedMediaType => String::from("unsupported_media_type"),
            Self::RangeNotSatisfiable => String::from("range_not_satisfiable"),
            Self::ExpectationFailed => String::from("expectation_failed"),
            Self::ImATeapot => String::from("im_a_teapot"),
            Self::MisdirectedRequest => String::from("misdirected_request"),
            Self::UnprocessableEntity => String::from("unprocessable_entity"),
            Self::Locked => String::from("locked"),
            Self::FailedDependency => String::from("failed_dependency"),
            Self::UpgradeRequired => String::from("upgrade_required"),
            Self::PreconditionRequired => String::from("precondition_required"),
            Self::TooManyRequests => String::from("too_many_requests"),
            Self::RequestHeaderFieldsTooLarge => String::from("request_header_fields_too_large"),
            Self::UnavailableForLegalReasons => String::from("unavailable_for_legal_reasons"),
            Self::InternalServerError => String::from("internal_server_error"),
            Self::NotImplemented => String::from("not_implemented"),
            Self::BadGateway => String::from("bad_gateway"),
            Self::ServiceUnavailable => String::from("service_unavailable"),
            Self::GatewayTimeout => String::from("gateway_timeout"),
            Self::HttpVersionNotSupported => String::from("http_version_not_supported"),
            Self::VariantAlsoNegotiates => String::from("variant_also_negotiates"),
            Self::InsufficientStorage => String::from("insufficient_storage"),
            Self::LoopDetected => String::from("loop_detected"),
            Self::NotExtended => String::from("not_extended"),
            Self::NetworkAuthenticationRequired => String::from("network_authentication_required"),
        };
        write!(f, "{}", str)
    }
}
