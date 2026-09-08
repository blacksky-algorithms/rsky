#[derive(Debug, Clone)]
pub enum ModerateOutcome {
    Applied,
    Failed {
        error: &'static str,
        message: String,
    },
}
