use crate::lexicon::lexicons::Root;
use lazy_static::lazy_static;

lazy_static! {
    // Deserializing Root needs more stack than tokio worker and test
    // threads provide, so parse on a dedicated thread
    pub static ref LEXICONS: Box<Root> = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            Box::new(
                toml::from_str::<Root>(include_str!("lexicons.toml"))
                    .expect("Failed to deserialize lexicons.toml"),
            )
        })
        .expect("Failed to spawn lexicon parser thread")
        .join()
        .expect("Lexicon parser thread panicked");
}

pub mod lexicons;
