use crate::models::gsc::TokenState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
pub mod cannibalization;
pub mod clarity;
pub mod content;
pub mod engine;
pub mod executor;
pub mod gsc;
pub mod health;
pub mod investigate;
pub mod live_site;
pub mod logging;
pub mod projects;
pub mod reddit;
pub mod seo;
pub mod settings;
pub mod skills;
pub mod social;
pub mod tasks;

pub use articles::*;
pub use cannibalization::*;
pub use clarity::*;
pub use content::*;
pub use engine::*;
pub use executor::*;
pub use gsc::*;
pub use health::*;
pub use investigate::*;
pub use live_site::*;
pub use logging::*;
pub use projects::*;
pub use reddit::*;
pub use seo::*;
pub use settings::*;
pub use skills::*;
pub use social::*;
pub use tasks::*;
