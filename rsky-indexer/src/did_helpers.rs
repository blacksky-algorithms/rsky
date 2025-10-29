use rsky_identity::types::DidDocument;

/// Extract handle from DID document's alsoKnownAs field
/// Looks for entries starting with "at://" and strips the prefix
pub fn get_handle(doc: &DidDocument) -> Option<String> {
    if let Some(ref aka) = doc.also_known_as {
        for alias in aka {
            if alias.starts_with("at://") {
                // Strip off "at://" prefix
                return Some(alias[5..].to_string());
            }
        }
    }
    None
}

/// Extract PDS endpoint from DID document's services
/// Looks for service with id "#atproto_pds" and type "AtprotoPersonalDataServer"
pub fn get_pds_endpoint(doc: &DidDocument) -> Option<String> {
    if let Some(ref services) = doc.service {
        for service in services {
            // Check if the ID matches (can be "#atproto_pds" or "did:...#atproto_pds")
            let id_matches = service.id == "#atproto_pds"
                || service.id.ends_with("#atproto_pds");

            if id_matches && service.r#type == "AtprotoPersonalDataServer" {
                // Validate URL
                if service.service_endpoint.starts_with("http://")
                    || service.service_endpoint.starts_with("https://")
                {
                    return Some(service.service_endpoint.clone());
                }
            }
        }
    }
    None
}

/// Extract signing key from DID document's verification methods
/// Looks for verification method with id "#atproto"
pub fn get_signing_key(doc: &DidDocument) -> Option<String> {
    if let Some(ref methods) = doc.verification_method {
        for method in methods {
            // Check if the ID matches (can be "#atproto" or "did:...#atproto")
            let id_matches = method.id == "#atproto" || method.id.ends_with("#atproto");

            if id_matches {
                if let Some(ref key) = method.public_key_multibase {
                    return Some(key.clone());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsky_identity::types::{Service, VerificationMethod};

    #[test]
    fn test_get_handle() {
        let mut doc = DidDocument {
            context: None,
            id: "did:plc:test".to_string(),
            also_known_as: None,
            verification_method: None,
            service: None,
        };

        // Test with no alsoKnownAs
        assert_eq!(get_handle(&doc), None);

        // Test with valid handle
        doc.also_known_as = Some(vec!["at://alice.bsky.social".to_string()]);
        assert_eq!(get_handle(&doc), Some("alice.bsky.social".to_string()));

        // Test with multiple entries
        doc.also_known_as = Some(vec![
            "https://example.com".to_string(),
            "at://bob.test".to_string(),
        ]);
        assert_eq!(get_handle(&doc), Some("bob.test".to_string()));
    }

    #[test]
    fn test_get_pds_endpoint() {
        let mut doc = DidDocument {
            context: None,
            id: "did:plc:test".to_string(),
            also_known_as: None,
            verification_method: None,
            service: None,
        };

        // Test with no services
        assert_eq!(get_pds_endpoint(&doc), None);

        // Test with valid PDS service
        doc.service = Some(vec![Service {
            id: "#atproto_pds".to_string(),
            r#type: "AtprotoPersonalDataServer".to_string(),
            service_endpoint: "https://bsky.social".to_string(),
        }]);
        assert_eq!(
            get_pds_endpoint(&doc),
            Some("https://bsky.social".to_string())
        );

        // Test with full DID in service ID
        doc.service = Some(vec![Service {
            id: "did:plc:test#atproto_pds".to_string(),
            r#type: "AtprotoPersonalDataServer".to_string(),
            service_endpoint: "https://pds.example.com".to_string(),
        }]);
        assert_eq!(
            get_pds_endpoint(&doc),
            Some("https://pds.example.com".to_string())
        );
    }

    #[test]
    fn test_get_signing_key() {
        let mut doc = DidDocument {
            context: None,
            id: "did:plc:test".to_string(),
            also_known_as: None,
            verification_method: None,
            service: None,
        };

        // Test with no verification methods
        assert_eq!(get_signing_key(&doc), None);

        // Test with valid signing key
        doc.verification_method = Some(vec![VerificationMethod {
            id: "#atproto".to_string(),
            r#type: "Multikey".to_string(),
            controller: "did:plc:test".to_string(),
            public_key_multibase: Some("zQ3shtest".to_string()),
        }]);
        assert_eq!(get_signing_key(&doc), Some("zQ3shtest".to_string()));
    }
}
