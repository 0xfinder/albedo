//! Shared runtime state passed to the dispatcher and background tasks.

use std::sync::Arc;

use teloxide::prelude::Bot;

use crate::config::Config;
use crate::db::Db;

/// Shared runtime state. New globals go here instead of growing the argument
/// lists of `bot::start`, the monitoring spawners, and the handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    pub bot: Bot,
    pub db: Db,
    pub config: Arc<Config>,
}
