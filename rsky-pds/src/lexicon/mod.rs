use crate::lexicon::lexicons::Root;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref LEXICONS: Root = {
        let cargo_toml: Root =
            toml::from_str(include_str!("lexicons.toml")).expect("Failed to deserialize lexicons.toml");
        cargo_toml
    };
}

pub mod lexicons;

#[cfg(test)]
mod tests {
    use super::LEXICONS;

    #[test]
    fn loads_lexicons_without_runtime_filesystem_dependency() {
        std::thread::Builder::new()
            .name("lexicons-load".to_string())
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let _ = &LEXICONS.com_atproto_repo_put_record;
            })
            .expect("failed to spawn lexicon loader thread")
            .join()
            .expect("lexicon loader thread panicked");
    }
}
