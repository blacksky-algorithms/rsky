use std::env;

pub fn env_int(name: &str) -> Option<usize> {
    match env::var(name) {
        Ok(str) => match str.parse::<usize>() {
            Ok(int) => Some(int),
            _ => None,
        },
        _ => None,
    }
}

pub fn env_str(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(str) => Some(str),
        _ => None,
    }
}

pub fn env_bool(name: &str) -> Option<bool> {
    match env::var(name) {
        Ok(str) if str == "true" || str == "1" => Some(true),
        Ok(str) if str == "false" || str == "0" => Some(false),
        _ => None,
    }
}

pub fn env_list(name: &str) -> Vec<String> {
    match env::var(name) {
        Ok(str) => str.split(",").into_iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}
