//! The `Recipe` trait + supporting types.

use thiserror::Error;

use crate::recipes::context::schema::RecipeContext;
use crate::recipes::context::StoreError;
use crate::recipes::recurrence::RecurrenceError;
use crate::recipes::trigger::Trigger;
use crate::types::InstanceId;

/// Permission that a Recipe declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipePermission {
    /// Android permission string, e.g. `"android.permission.POST_NOTIFICATIONS"`.
    pub name: &'static str,
    /// Why the Recipe needs this permission (for documentation and UI).
    pub rationale: &'static str,
    /// Whether the Recipe degrades gracefully without this permission.
    pub required: bool,
    /// Minimum API level where this permission exists; `None` for legacy.
    pub min_api: Option<u32>,
}

/// Errors a Recipe operation may produce.
///
/// This is the single error type for the recipe layer: the dispatch boundary
/// ([`crate::recipes::dispatch_trigger`]) and the Recipe handlers both return
/// it, so there is no separate "dispatch error" wrapper.
#[derive(Debug, Error)]
pub enum RecipeError {
    /// A required field on the Context was malformed for the Recipe's
    /// state machine.
    #[error("invalid context for {recipe}: {message}")]
    InvalidContext {
        recipe: &'static str,
        message: String,
    },
    /// No persisted Context exists for the dispatched instance id.
    #[error("no context for instance {0}")]
    ContextNotFound(InstanceId),
    /// The Context's `recipe_type` tag is not registered in the recipe
    /// registry.
    #[error("recipe '{0}' not registered")]
    RecipeNotRegistered(String),
    /// The recurrence engine reported an error computing next_fire.
    #[error("recurrence error: {0}")]
    Recurrence(#[from] RecurrenceError),
    /// ContextStore I/O or schema failure.
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    /// The transition is illegal in the current state (e.g., dismissing
    /// an Idle instance).
    #[error("illegal transition: {message}")]
    IllegalTransition { message: String },
    /// Recipe-defined error.
    #[error("recipe error: {0}")]
    Other(String),
}

/// A Behavior Recipe.
///
/// All Recipe types must be `Send + Sync + 'static` so the runtime can
/// hold them in a global registry behind an `Arc<dyn Recipe>`.
pub trait Recipe: Send + Sync + 'static {
    /// Stable lower-snake-case identifier — must match the
    /// [`RecipeContext`]'s tag for the Recipe's variant.
    fn recipe_type(&self) -> &'static str;

    /// Android permissions the Recipe requires.
    fn required_permissions(&self) -> &'static [RecipePermission] {
        &[]
    }

    /// Handle a Trigger. Called by `dispatch_trigger` AFTER LoadContext-
    /// on-every-trigger has been enforced — the `context` parameter is a
    /// freshly-loaded Context for the affected instance.
    fn handle_trigger(
        &self,
        trigger: Trigger,
        instance_id: &InstanceId,
        context: &RecipeContext,
    ) -> Result<(), RecipeError>;
}
