/// Centralised path resolution for a project.
///
/// All engine code that needs a file path should derive it from here so that
/// the layout (`repo_root/.github/automation/…`) is defined in exactly one
/// place and never re-implemented ad-hoc in handlers or commands.
///
/// # Layout
/// ```text
/// <repo_root>/
///   .github/
///     automation/
///       articles.json
///       task_list.json
///       artifacts/
///       task_results/
///       reddit/
///       manifest.json
///       project.md
///       reddit_config.md
/// ```

use std::path::{Path, PathBuf};

use crate::models::project::Project;

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    /// The repository root — the value stored in `projects.path`.
    pub repo_root: PathBuf,
    /// `.github/automation/` — the automation workspace.
    pub automation_dir: PathBuf,
    /// `automation_dir/articles.json`
    pub articles_json: PathBuf,
    /// `automation_dir/task_list.json`
    pub task_list_json: PathBuf,
    /// `automation_dir/artifacts/`
    pub artifacts_dir: PathBuf,
    /// `automation_dir/task_results/`
    pub task_results_dir: PathBuf,
    /// `automation_dir/reddit/`
    pub reddit_dir: PathBuf,
}

impl ProjectPaths {
    /// Derive all paths from a project record.
    pub fn from_project(project: &Project) -> Self {
        Self::from_path(&project.path)
    }

    /// Derive all paths from a raw repo-root string.
    pub fn from_path(repo_root: &str) -> Self {
        let repo_root = PathBuf::from(repo_root);
        let automation_dir = repo_root.join(".github").join("automation");
        Self {
            articles_json: automation_dir.join("articles.json"),
            task_list_json: automation_dir.join("task_list.json"),
            artifacts_dir: automation_dir.join("artifacts"),
            task_results_dir: automation_dir.join("task_results"),
            reddit_dir: automation_dir.join("reddit"),
            automation_dir,
            repo_root,
        }
    }

    /// Return the automation_dir as a `&Path`.
    pub fn automation_dir(&self) -> &Path {
        &self.automation_dir
    }

    /// Return the repo_root as a `&Path`.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Resolve a path relative to the automation directory.
    pub fn in_automation(&self, rel: &str) -> PathBuf {
        self.automation_dir.join(rel)
    }

    /// Return the project directory (alias for repo_root).
    pub fn project_dir(&self) -> &Path {
        &self.repo_root
    }

    /// Return the social media output directory.
    pub fn social_output_dir(&self) -> PathBuf {
        self.automation_dir.join("social")
    }
}

/// Apply standard path substitutions to a command template string.
///
/// Supported tokens:
/// - `{project_path}` → `repo_root` absolute path
/// - `{automation_dir}` → `repo_root/.github/automation` absolute path
pub fn apply_path_substitutions(cmd: &str, paths: &ProjectPaths) -> String {
    cmd.replace("{project_path}", &paths.repo_root.to_string_lossy())
        .replace("{automation_dir}", &paths.automation_dir.to_string_lossy())
}
