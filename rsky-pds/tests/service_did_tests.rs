use rsky_pds::config::{env_to_cfg, service_did};

#[test]
fn service_did_defaults_to_did_web_hostname_and_honours_override() {
    std::env::remove_var("PDS_SERVICE_DID");
    std::env::set_var("PDS_HOSTNAME", "pds.example.com");
    assert_eq!(service_did(), "did:web:pds.example.com");
    assert_eq!(env_to_cfg().service.did, service_did());

    std::env::remove_var("PDS_HOSTNAME");
    assert_eq!(service_did(), "did:web:localhost");

    std::env::set_var("PDS_SERVICE_DID", "did:plc:override");
    assert_eq!(service_did(), "did:plc:override");
    assert_eq!(env_to_cfg().service.did, "did:plc:override");
    std::env::remove_var("PDS_SERVICE_DID");
}
