use crate::lexicon::lexicons::Root;
use lazy_static::lazy_static;
use std::fs;

lazy_static! {
    pub static ref LEXICONS: Root = {
        let toml_str = fs::read_to_string("lexicons.toml").expect("Failed to open lexicon file");
        let cargo_toml: Root =
            toml::from_str(&toml_str).expect("Failed to deserialize lexicons.toml");
        cargo_toml
    };
}

pub mod lexicons;
