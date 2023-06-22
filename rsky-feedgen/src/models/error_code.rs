#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "no_error")]
    NoError,
    #[serde(rename = "validation_error")]
    ValidationError,
    #[serde(rename = "authorization_model_not_found")]
    AuthorizationModelNotFound,
    #[serde(rename = "authorization_model_resolution_too_complex")]
    AuthorizationModelResolutionTooComplex,
    #[serde(rename = "invalid_write_input")]
    InvalidWriteInput,
    #[serde(rename = "cannot_allow_duplicate_tuples_in_one_request")]
    CannotAllowDuplicateTuplesInOneRequest,
    #[serde(rename = "cannot_allow_duplicate_types_in_one_request")]
    CannotAllowDuplicateTypesInOneRequest,
    #[serde(rename = "cannot_allow_multiple_references_to_one_relation")]
    CannotAllowMultipleReferencesToOneRelation,
    #[serde(rename = "invalid_continuation_token")]
    InvalidContinuationToken,
    #[serde(rename = "invalid_tuple_set")]
    InvalidTupleSet,
    #[serde(rename = "invalid_check_input")]
    InvalidCheckInput,
    #[serde(rename = "invalid_expand_input")]
    InvalidExpandInput,
    #[serde(rename = "unsupported_user_set")]
    UnsupportedUserSet,
    #[serde(rename = "invalid_object_format")]
    InvalidObjectFormat,
    #[serde(rename = "write_failed_due_to_invalid_input")]
    WriteFailedDueToInvalidInput,
    #[serde(rename = "authorization_model_assertions_not_found")]
    AuthorizationModelAssertionsNotFound,
    #[serde(rename = "latest_authorization_model_not_found")]
    LatestAuthorizationModelNotFound,
    #[serde(rename = "type_not_found")]
    TypeNotFound,
    #[serde(rename = "relation_not_found")]
    RelationNotFound,
    #[serde(rename = "empty_relation_definition")]
    EmptyRelationDefinition,
    #[serde(rename = "invalid_user")]
    InvalidUser,
    #[serde(rename = "invalid_tuple")]
    InvalidTuple,
    #[serde(rename = "unknown_relation")]
    UnknownRelation,
    #[serde(rename = "store_id_invalid_length")]
    StoreIdInvalidLength,
    #[serde(rename = "assertions_too_many_items")]
    AssertionsTooManyItems,
    #[serde(rename = "id_too_long")]
    IdTooLong,
    #[serde(rename = "authorization_model_id_too_long")]
    AuthorizationModelIdTooLong,
    #[serde(rename = "tuple_key_value_not_specified")]
    TupleKeyValueNotSpecified,
    #[serde(rename = "tuple_keys_too_many_or_too_few_items")]
    TupleKeysTooManyOrTooFewItems,
    #[serde(rename = "page_size_invalid")]
    PageSizeInvalid,
    #[serde(rename = "param_missing_value")]
    ParamMissingValue,
    #[serde(rename = "difference_base_missing_value")]
    DifferenceBaseMissingValue,
    #[serde(rename = "subtract_base_missing_value")]
    SubtractBaseMissingValue,
    #[serde(rename = "object_too_long")]
    ObjectTooLong,
    #[serde(rename = "relation_too_long")]
    RelationTooLong,
    #[serde(rename = "type_definitions_too_few_items")]
    TypeDefinitionsTooFewItems,
    #[serde(rename = "type_invalid_length")]
    TypeInvalidLength,
    #[serde(rename = "type_invalid_pattern")]
    TypeInvalidPattern,
    #[serde(rename = "relations_too_few_items")]
    RelationsTooFewItems,
    #[serde(rename = "relations_too_long")]
    RelationsTooLong,
    #[serde(rename = "relations_invalid_pattern")]
    RelationsInvalidPattern,
    #[serde(rename = "object_invalid_pattern")]
    ObjectInvalidPattern,
    #[serde(rename = "query_string_type_continuation_token_mismatch")]
    QueryStringTypeContinuationTokenMismatch,
    #[serde(rename = "exceeded_entity_limit")]
    ExceededEntityLimit,
    #[serde(rename = "invalid_contextual_tuple")]
    InvalidContextualTuple,
    #[serde(rename = "duplicate_contextual_tuple")]
    DuplicateContextualTuple,
    #[serde(rename = "invalid_authorization_model")]
    InvalidAuthorizationModel,
    #[serde(rename = "unsupported_schema_version")]
    UnsupportedSchemaVersion,
}

impl ToString for ErrorCode {
    fn to_string(&self) -> String {
        match self {
            Self::NoError => String::from("no_error"),
            Self::ValidationError => String::from("validation_error"),
            Self::AuthorizationModelNotFound => String::from("authorization_model_not_found"),
            Self::AuthorizationModelResolutionTooComplex => {
                String::from("authorization_model_resolution_too_complex")
            }
            Self::InvalidWriteInput => String::from("invalid_write_input"),
            Self::CannotAllowDuplicateTuplesInOneRequest => {
                String::from("cannot_allow_duplicate_tuples_in_one_request")
            }
            Self::CannotAllowDuplicateTypesInOneRequest => {
                String::from("cannot_allow_duplicate_types_in_one_request")
            }
            Self::CannotAllowMultipleReferencesToOneRelation => {
                String::from("cannot_allow_multiple_references_to_one_relation")
            }
            Self::InvalidContinuationToken => String::from("invalid_continuation_token"),
            Self::InvalidTupleSet => String::from("invalid_tuple_set"),
            Self::InvalidCheckInput => String::from("invalid_check_input"),
            Self::InvalidExpandInput => String::from("invalid_expand_input"),
            Self::UnsupportedUserSet => String::from("unsupported_user_set"),
            Self::InvalidObjectFormat => String::from("invalid_object_format"),
            Self::WriteFailedDueToInvalidInput => String::from("write_failed_due_to_invalid_input"),
            Self::AuthorizationModelAssertionsNotFound => {
                String::from("authorization_model_assertions_not_found")
            }
            Self::LatestAuthorizationModelNotFound => {
                String::from("latest_authorization_model_not_found")
            }
            Self::TypeNotFound => String::from("type_not_found"),
            Self::RelationNotFound => String::from("relation_not_found"),
            Self::EmptyRelationDefinition => String::from("empty_relation_definition"),
            Self::InvalidUser => String::from("invalid_user"),
            Self::InvalidTuple => String::from("invalid_tuple"),
            Self::UnknownRelation => String::from("unknown_relation"),
            Self::StoreIdInvalidLength => String::from("store_id_invalid_length"),
            Self::AssertionsTooManyItems => String::from("assertions_too_many_items"),
            Self::IdTooLong => String::from("id_too_long"),
            Self::AuthorizationModelIdTooLong => String::from("authorization_model_id_too_long"),
            Self::TupleKeyValueNotSpecified => String::from("tuple_key_value_not_specified"),
            Self::TupleKeysTooManyOrTooFewItems => {
                String::from("tuple_keys_too_many_or_too_few_items")
            }
            Self::PageSizeInvalid => String::from("page_size_invalid"),
            Self::ParamMissingValue => String::from("param_missing_value"),
            Self::DifferenceBaseMissingValue => String::from("difference_base_missing_value"),
            Self::SubtractBaseMissingValue => String::from("subtract_base_missing_value"),
            Self::ObjectTooLong => String::from("object_too_long"),
            Self::RelationTooLong => String::from("relation_too_long"),
            Self::TypeDefinitionsTooFewItems => String::from("type_definitions_too_few_items"),
            Self::TypeInvalidLength => String::from("type_invalid_length"),
            Self::TypeInvalidPattern => String::from("type_invalid_pattern"),
            Self::RelationsTooFewItems => String::from("relations_too_few_items"),
            Self::RelationsTooLong => String::from("relations_too_long"),
            Self::RelationsInvalidPattern => String::from("relations_invalid_pattern"),
            Self::ObjectInvalidPattern => String::from("object_invalid_pattern"),
            Self::QueryStringTypeContinuationTokenMismatch => {
                String::from("query_string_type_continuation_token_mismatch")
            }
            Self::ExceededEntityLimit => String::from("exceeded_entity_limit"),
            Self::InvalidContextualTuple => String::from("invalid_contextual_tuple"),
            Self::DuplicateContextualTuple => String::from("duplicate_contextual_tuple"),
            Self::InvalidAuthorizationModel => String::from("invalid_authorization_model"),
            Self::UnsupportedSchemaVersion => String::from("unsupported_schema_version"),
        }
    }
}

impl Default for ErrorCode {
    fn default() -> ErrorCode {
        Self::NoError
    }
}
