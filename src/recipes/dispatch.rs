//! The single entry point for all Trigger dispatches.
//!
//! Enforces the LoadContext-on-every-trigger invariant: every dispatch
//! acquires the per-id ContextStore mutex and reloads the Context before
//! routing to the Recipe handler. Recipes never receive settings from any
//! source other than the freshly-loaded Context.
//!
//! See `requirements.md §Requirement 5.5, 7.5` and `design.md §1.3`.

use crate::recipes::context::{ContextStore, StoreError};
use crate::recipes::recipe::RecipeError;
use crate::recipes::registry::recipe_registry;
use crate::recipes::trigger::Trigger;
use crate::types::InstanceId;

/// Dispatch `trigger` for `instance_id` against the given store.
///
/// Steps:
/// 1. Load the Context (returns [`RecipeError::ContextNotFound`] if absent).
/// 2. Resolve the Recipe by `recipe_type` (returns
///    [`RecipeError::RecipeNotRegistered`] if unknown).
/// 3. Call `Recipe::handle_trigger` with the freshly-loaded Context.
///
/// The Recipe is responsible for any subsequent Context writes — they
/// must go through the same `ContextStore` and will pick up a new
/// monotonic revision.
pub fn dispatch_trigger(
    store: &ContextStore,
    trigger: Trigger,
    instance_id: &InstanceId,
) -> Result<(), RecipeError> {
    // Step 1: load Context. The ContextStore handles its own per-id
    // mutex internally on writes; reads observe the most recently
    // committed record. A missing record is a distinct, expected
    // condition (not a generic store I/O failure).
    let record = match store.load(instance_id) {
        Ok(r) => r,
        Err(StoreError::NotFound(id)) => return Err(RecipeError::ContextNotFound(id)),
        Err(e) => return Err(RecipeError::Store(e)),
    };

    // Step 2: locate Recipe.
    let recipe_type = record.context.recipe_type();
    let recipe = recipe_registry()
        .get(recipe_type)
        .ok_or_else(|| RecipeError::RecipeNotRegistered(recipe_type.to_owned()))?;

    // Step 3: hand the freshly-loaded Context to the Recipe.
    recipe.handle_trigger(trigger, instance_id, &record.context)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::context::schema::{
        AlarmClassContext, ContextRecord, RecipeContext, ScheduleSpec, SnoozePolicy, SoundIdSpec,
    };
    use crate::recipes::recipe::Recipe;
    use crate::recipes::registry::register_recipe;
    use crate::recipes::test_lock::lock_and_clear;
    use crate::recipes::trigger::Trigger;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn sample_context(label: &str) -> RecipeContext {
        RecipeContext::AlarmClass(AlarmClassContext {
            label: label.into(),
            schedule: ScheduleSpec::OneTime {
                date: "2026-06-01".into(),
                hour: 7,
                minute: 0,
            },
            time_zone: "UTC".into(),
            sound_id: SoundIdSpec::Bundled("happy".into()),
            snooze_policy: SnoozePolicy {
                max_count: 3,
                interval_minutes: 5,
                escalation: None,
            },
            challenges: vec![],
            vibration_enabled: false,
            vibration_pattern: None,
            kiosk_mode: false,
            bypass_dnd: false,
            snooze_count: 0,
            challenges_solved: false,
        })
    }

    /// Recipe that records the Trigger + Context label it received.
    struct CapturingRecipe {
        recipe_type: &'static str,
        seen_trigger: Arc<Mutex<Option<Trigger>>>,
        seen_label: Arc<Mutex<Option<String>>>,
        call_count: Arc<AtomicU32>,
    }
    impl Recipe for CapturingRecipe {
        fn recipe_type(&self) -> &'static str {
            self.recipe_type
        }
        fn handle_trigger(
            &self,
            trigger: Trigger,
            _: &InstanceId,
            context: &RecipeContext,
        ) -> Result<(), RecipeError> {
            *self.seen_trigger.lock().unwrap() = Some(trigger);
            if let RecipeContext::AlarmClass(c) = context {
                *self.seen_label.lock().unwrap() = Some(c.label.clone());
            }
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn dispatch_loads_context_and_invokes_recipe() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());

        let seen_trigger = Arc::new(Mutex::new(None));
        let seen_label = Arc::new(Mutex::new(None));
        let count = Arc::new(AtomicU32::new(0));
        register_recipe(CapturingRecipe {
            recipe_type: "alarm_class",
            seen_trigger: seen_trigger.clone(),
            seen_label: seen_label.clone(),
            call_count: count.clone(),
        })
        .unwrap();

        let id = InstanceId::new("alarm-1");
        store
            .write(ContextRecord::new(id.clone(), sample_context("Wake up")))
            .unwrap();

        dispatch_trigger(&store, Trigger::Fire, &id).unwrap();
        assert_eq!(*seen_trigger.lock().unwrap(), Some(Trigger::Fire));
        assert_eq!(seen_label.lock().unwrap().as_deref(), Some("Wake up"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn loadcontext_invariant_observed_after_edit() {
        // Property: an Edit that mutates the Context must be visible to
        // the very next dispatch, regardless of what the previous
        // dispatch saw. This is the exact bug class the migration fixes.
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());

        let seen_label = Arc::new(Mutex::new(None));
        let count = Arc::new(AtomicU32::new(0));
        register_recipe(CapturingRecipe {
            recipe_type: "alarm_class",
            seen_trigger: Arc::new(Mutex::new(None)),
            seen_label: seen_label.clone(),
            call_count: count.clone(),
        })
        .unwrap();

        let id = InstanceId::new("alarm-1");
        store
            .write(ContextRecord::new(id.clone(), sample_context("First")))
            .unwrap();
        dispatch_trigger(&store, Trigger::Fire, &id).unwrap();
        assert_eq!(seen_label.lock().unwrap().as_deref(), Some("First"));

        // Simulate Edit by overwriting the Context.
        store
            .write(ContextRecord::new(id.clone(), sample_context("Second")))
            .unwrap();

        // Next dispatch sees the new value — LoadContext-on-every-trigger.
        dispatch_trigger(&store, Trigger::Fire, &id).unwrap();
        assert_eq!(seen_label.lock().unwrap().as_deref(), Some("Second"));
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn dispatch_for_missing_instance_returns_not_found() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let err = dispatch_trigger(&store, Trigger::Fire, &InstanceId::new("ghost")).unwrap_err();
        assert!(matches!(err, RecipeError::ContextNotFound(_)));
    }

    #[test]
    fn dispatch_for_unregistered_recipe_returns_error() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        store
            .write(ContextRecord::new(id.clone(), sample_context("L")))
            .unwrap();
        let err = dispatch_trigger(&store, Trigger::Fire, &id).unwrap_err();
        assert!(matches!(err, RecipeError::RecipeNotRegistered(_)));
    }

    #[test]
    fn recipe_error_propagates() {
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());

        struct FailingRecipe;
        impl Recipe for FailingRecipe {
            fn recipe_type(&self) -> &'static str {
                "alarm_class"
            }
            fn handle_trigger(
                &self,
                _: Trigger,
                _: &InstanceId,
                _: &RecipeContext,
            ) -> Result<(), RecipeError> {
                Err(RecipeError::Other("boom".into()))
            }
        }
        register_recipe(FailingRecipe).unwrap();
        let id = InstanceId::new("a");
        store
            .write(ContextRecord::new(id.clone(), sample_context("L")))
            .unwrap();
        let err = dispatch_trigger(&store, Trigger::Fire, &id).unwrap_err();
        assert!(matches!(err, RecipeError::Other(_)));
    }

    #[test]
    fn dispatch_does_not_consume_action_variant_unrelated_to_loadcontext() {
        // Sanity: the dispatch boundary is generic over Triggers — it
        // doesn't special-case Fire vs Snooze vs Dismiss.
        let _g = lock_and_clear();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());

        let count = Arc::new(AtomicU32::new(0));
        register_recipe(CapturingRecipe {
            recipe_type: "alarm_class",
            seen_trigger: Arc::new(Mutex::new(None)),
            seen_label: Arc::new(Mutex::new(None)),
            call_count: count.clone(),
        })
        .unwrap();
        let id = InstanceId::new("a");
        store
            .write(ContextRecord::new(id.clone(), sample_context("L")))
            .unwrap();
        for trigger in [Trigger::Fire, Trigger::Snooze, Trigger::Dismiss] {
            dispatch_trigger(&store, trigger, &id).unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }
}
