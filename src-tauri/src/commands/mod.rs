use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use crate::models::gsc::TokenState;

pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub db_path: PathBuf,
}

pub struct GscState {
    pub token: Mutex<Option<TokenState>>,
}

pub struct SeoState {
    pub sig_cache: Mutex<std::collections::HashMap<String, crate::seo::backlinks::CachedSignature>>,
}

pub mod articles;
pub mod content;
pub mod engine;
pub mod gsc;
pub mod projects;
pub mod reddit;
pub mod seo;
pub mod settings;
pub mod skills;
pub mod tasks;

pub use articles::*;
pub use content::*;
pub use engine::*;
pub use gsc::*;
pub use projects::*;
pub use reddit::*;
pub use seo::*;
pub use settings::*;
pub use skills::*;
pub use tasks::*;
