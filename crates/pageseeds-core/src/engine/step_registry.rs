#[macro_use]
mod macros;
mod fix_content;
mod gsc_indexing;
mod registry;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use rusqlite::Connection;

use crate::engine::workflows::{StepKind, StepResult, WorkflowStep};
use crate::models::task::Task;

pub struct StepContext<'a> {
    pub task: &'a Task,
    pub project_path: &'a str,
    pub site_url: &'a str,
    pub agent_provider: &'a str,
    pub seo_provider: &'a str,
    pub latest_raw: Option<&'a str>,
    pub gsc_token: Option<&'a str>,
    pub conn: &'a Connection,
}

type HandlerFn = Box<
    dyn for<'b> Fn(
            &'b WorkflowStep,
            &'b StepContext<'b>,
        ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'b>>
        + Send
        + Sync,
>;

pub struct StepRegistry {
    handlers: HashMap<StepKind, HandlerFn>,
}

impl StepRegistry {
    pub fn new() -> Self {
        Self {
            handlers: registry::build_handlers(),
        }
    }

    pub fn get(&self, kind: &StepKind) -> Option<&HandlerFn> {
        self.handlers.get(kind)
    }
}

impl Default for StepRegistry {
    fn default() -> Self {
        Self::new()
    }
}
