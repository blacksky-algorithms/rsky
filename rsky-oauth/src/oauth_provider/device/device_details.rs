use rocket::Request;
use std::net::IpAddr;

pub struct DeviceDetails {
    pub user_agent: Option<String>,
    pub ip_address: IpAddr,
}

pub fn extract_device_details(req: &Request) -> DeviceDetails {
    let user_agent = req.headers().get("user-agent").next().unwrap().to_string();
    let ip_address = req.client_ip().unwrap();

    DeviceDetails {
        user_agent: Some(user_agent),
        ip_address,
    }
}
