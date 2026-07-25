/// Project setup validation — single source of truth for understanding a project's state.
///
/// Every piece of engine code that needs to locate content, articles.json, or the
/// automation workspace should go through `ProjectSetup::resolve()`.  The returned
/// struct is fully serialisable so it can be sent directly to the UI, which can
/// then surface actionable warnings to the user.
///
/// # Workspace layout expected
/// ```text
/// <repo_root>/
///   .github/
///     automation/
///       articles.json          ← required for content workflows
///       task_list.json
///       seo_workspace.json     ← optional but strongly recommended; pins content_dir
/// ```
///
/// # seo_workspace.json format
/// ```json
/// {
///   "content_dir": "src/blog/posts",
///   "site_url": "https://example.com"
/// }
/// ```
///  `content_dir` is a path relative to `repo_root` (or absolute).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};


mod types;
mod config_status;
mod resolution;
mod checks;
mod helpers;
mod init;

pub use types::*;
pub use config_status::*;
pub use resolution::*;
pub use checks::*;
pub use helpers::*;
pub use init::*;
