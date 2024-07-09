pub fn nodejs_format(format: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut result = String::new();
    let parts = format.split("{}");
    for (i, part) in parts.enumerate() {
        result.push_str(part);
        if i < args.len() {
            result.push_str(&args[i].to_string());
        }
    }
    result
}
