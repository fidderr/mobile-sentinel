//! Global registry of registered Recipes.
//!
//! The registry lives behind an `Arc<RwLock<...>>` once-cell so dispatch
//! can `read()` cheaply on every Trigger and registration is one-shot at
//! startup.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use once_cell::sync::Lazy;
use thiserror::Error;

use super::recipe::Recipe;

/// Errors raised at registration time.
#[derive(Debug, Error)]
pub enum RegistrationError {
    /// A Recipe with the same `recipe_type` is already registered.
    #[error("recipe '{0}' already registered")]
    DuplicateRecipe(String),
}

/// The global Recipe registry. Keyed by `Recipe::recipe_type()`.
pub struct RecipeRegistry {
    inner: RwLock<HashMap<&'static str, Arc<dyn Recipe>>>,
}

impl RecipeRegistry {
    fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Look up a registered Recipe by type tag.
    pub fn get(&self, recipe_type: &str) -> Option<Arc<dyn Recipe>> {
        self.inner.read().unwrap().get(recipe_type).cloned()
    }

    /// Snapshot of every registered Recipe's `recipe_type`.
    pub fn registered_types(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.inner.read().unwrap().keys().copied().collect();
        v.sort();
        v
    }

    /// Test-only: clear the registry.
    #[cfg(test)]
    pub(crate) fn clear(&self) {
        self.inner.write().unwrap().clear();
    }
}

/// Process-global registry singleton.
static REGISTRY: Lazy<RecipeRegistry> = Lazy::new(RecipeRegistry::new);

/// Borrow the global registry.
pub fn recipe_registry() -> &'static RecipeRegistry {
    &REGISTRY
}

/// Register a Recipe. Rejects a duplicate `recipe_type`. All-or-nothing:
/// if the type is already registered, the registry is unchanged.
pub fn register_recipe<R: Recipe>(recipe: R) -> Result<(), RegistrationError> {
    let recipe_type = recipe.recipe_type();
    let mut guard = REGISTRY.inner.write().unwrap();
    if guard.contains_key(recipe_type) {
        return Err(RegistrationError::DuplicateRecipe(recipe_type.to_owned()));
    }
    guard.insert(recipe_type, Arc::new(recipe));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::context::schema::RecipeContext;
    use crate::recipes::recipe::{Recipe, RecipeError};
    use crate::recipes::test_lock::lock_and_clear;
    use crate::recipes::trigger::Trigger;
    use crate::types::InstanceId;

    struct DummyRecipe {
        recipe_type: &'static str,
    }
    impl Recipe for DummyRecipe {
        fn recipe_type(&self) -> &'static str {
            self.recipe_type
        }
        fn handle_trigger(
            &self,
            _: Trigger,
            _: &InstanceId,
            _: &RecipeContext,
        ) -> Result<(), RecipeError> {
            Ok(())
        }
    }

    #[test]
    fn register_and_lookup_round_trip() {
        let _g = lock_and_clear();
        register_recipe(DummyRecipe {
            recipe_type: "alarm_class",
        })
        .unwrap();
        let r = recipe_registry().get("alarm_class").expect("registered");
        assert_eq!(r.recipe_type(), "alarm_class");
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let _g = lock_and_clear();
        register_recipe(DummyRecipe {
            recipe_type: "alarm_class",
        })
        .unwrap();
        let err = register_recipe(DummyRecipe {
            recipe_type: "alarm_class",
        })
        .unwrap_err();
        assert!(matches!(err, RegistrationError::DuplicateRecipe(_)));
    }

    #[test]
    fn registered_types_returns_sorted_list() {
        let _g = lock_and_clear();
        register_recipe(DummyRecipe {
            recipe_type: "z_recipe",
        })
        .unwrap();
        register_recipe(DummyRecipe {
            recipe_type: "a_recipe",
        })
        .unwrap();
        let types = recipe_registry().registered_types();
        assert_eq!(types, vec!["a_recipe", "z_recipe"]);
    }
}
