//! Recipe model — pre-built, generic behavior bundles.
//!
//! Each Recipe encapsulates a state machine driven by Triggers, the
//! Context schema variant it operates on, and the Android permissions it
//! needs. Consumers compose products by registering Recipes with the
//! runtime and driving them through `dispatch_trigger`.
//!
//! # Layout
//!
//! - **Recipe engine** (the `recipes` feature): [`trigger`] (the `Trigger`
//!   catalogue), [`recipe`] (the `Recipe` trait), [`dispatch`], [`registry`],
//!   [`context`] (the typed `ContextStore`), and the recipe-shared helpers
//!   [`recurrence`] (DST-correct next-fire) and [`snooze`] (snooze policy).
//!   This is the reusable building block any recipe is built on — a consumer
//!   can implement its own `Recipe` against it without pulling in AlarmKit.
//! - **Prebuilt recipes** (one folder each, each its own feature): [`alarm_kit`]
//!   (the AlarmKit façade + its `alarm_class` state machine), behind
//!   `alarm-kit`. These own no engine logic — they compose the engine above
//!   with the firing / sound / jobs building blocks. Additional recipes live
//!   in their own folders alongside it.

// ---- Recipe engine (the `recipes` feature) ----
// The state-machine framework + the recipe-shared helpers
// (context/recurrence/snooze). The typed `context` store is the recipe
// engine's specialization of the generic `state-store` feature
// (`ContextStore = StateStore<ContextRecord>`); recurrence is used by the
// `Recipe` trait and recipes like AlarmClass; snooze is embedded in the shared
// context schema — so all live at the recipe-engine root, not inside a single
// recipe.
#[cfg(feature = "recipes")]
pub mod context;
#[cfg(feature = "recipes")]
pub mod dispatch;
#[cfg(feature = "recipes")]
pub mod recipe;
#[cfg(feature = "recipes")]
pub mod recurrence;
#[cfg(feature = "recipes")]
pub mod registry;
#[cfg(feature = "recipes")]
pub mod snooze;
#[cfg(feature = "recipes")]
pub mod trigger;

#[cfg(feature = "recipes")]
pub use trigger::Trigger;

// ---- Prebuilt recipes (one folder each) ----
#[cfg(feature = "alarm-kit")]
pub mod alarm_kit;

#[cfg(all(test, feature = "recipes"))]
mod test_lock;

// `alarm_class` lives inside the `alarm_kit` recipe folder; re-export it at
// the recipe-layer root so the public path `mobile_sentinel::recipes::alarm_class`
// stays stable (consumers use `alarm_class::{AlarmClassConfig, SoundResolver}`).
#[cfg(feature = "alarm-kit")]
pub use alarm_kit::alarm_class;

#[cfg(feature = "alarm-kit")]
pub use alarm_class::{
    alarm_class_runtime, handle_trigger_with_store, AlarmClass, AlarmClassRuntime,
};
#[cfg(feature = "alarm-kit")]
pub use alarm_kit::{AlarmKit, AlarmKitConfig, AlarmKitError, AlarmKitSession, AlarmSpec};

// ---- Recipe-engine re-exports (the `recipes` feature) ----
#[cfg(feature = "recipes")]
pub use dispatch::dispatch_trigger;
#[cfg(feature = "recipes")]
pub use recipe::{Recipe, RecipeError, RecipePermission};
#[cfg(feature = "recipes")]
pub use registry::{recipe_registry, register_recipe, RegistrationError};
