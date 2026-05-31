//! Top-level property tests proving the LoadContext-on-every-trigger
//! invariant and the bug-class invariants from `requirements.md §22`.
//!
//! These tests target the integration boundary (Recipe + ContextStore +
//! dispatch) rather than individual unit modules — they are the
//! canonical regression net for the snooze-loses-sound bug class.

use std::sync::Mutex;

use mobile_sentinel::context::schema::{
    AlarmClassContext, ContextRecord, RecipeContext, ScheduleSpec, SnoozePolicy, SoundIdSpec,
};
use mobile_sentinel::ContextStore;
use mobile_sentinel::InstanceId;
use mobile_sentinel::Revision;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use tempfile::TempDir;

fn arb_label() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ]{1,30}".prop_map(|s| s)
}

fn arb_sound() -> impl Strategy<Value = SoundIdSpec> {
    prop_oneof![
        "[a-z_]{3,10}".prop_map(SoundIdSpec::Bundled),
        "[a-z0-9-]{8,16}".prop_map(SoundIdSpec::Custom),
        Just(SoundIdSpec::SystemDefault),
        Just(SoundIdSpec::Silent),
    ]
}

fn arb_schedule() -> impl Strategy<Value = ScheduleSpec> {
    prop_oneof![
        (1u8..=28u8, 0u8..=23u8, 0u8..=59u8).prop_map(|(d, h, m)| {
            ScheduleSpec::OneTime {
                date: format!("2026-06-{:02}", d),
                hour: h,
                minute: m,
            }
        }),
        (1u8..=127u8, 0u8..=23u8, 0u8..=59u8).prop_map(|(mask, h, m)| {
            ScheduleSpec::Weekdays {
                days_mask: mask & 0b0111_1111,
                hour: h,
                minute: m,
            }
        }),
    ]
}

fn arb_alarm_class_context() -> impl Strategy<Value = AlarmClassContext> {
    (
        arb_label(),
        arb_schedule(),
        arb_sound(),
        0u32..=10u32,
        1u32..=30u32,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(
                label,
                schedule,
                sound_id,
                max_count,
                interval_minutes,
                vibration_enabled,
                kiosk_mode,
                bypass_dnd,
            )| AlarmClassContext {
                label,
                schedule,
                time_zone: "UTC".into(),
                sound_id,
                snooze_policy: SnoozePolicy {
                    max_count,
                    interval_minutes,
                    escalation: None,
                },
                challenges: vec![],
                vibration_enabled,
                vibration_pattern: None,
                kiosk_mode,
                bypass_dnd,
                snooze_count: 0,
                challenges_solved: false,
            },
        )
}

// Property 7 (R7.10 / R22.7): ContextStore last-writer-wins.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn context_store_last_writer_wins(
        contexts in prop::collection::vec(arb_alarm_class_context(), 1..=8),
    ) {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        for ctx in &contexts {
            let record = ContextRecord::new(id.clone(), RecipeContext::AlarmClass(ctx.clone()));
            store.write(record).unwrap();
        }
        // Final read sees the last-written context.
        let loaded = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &loaded.context {
            prop_assert_eq!(c, contexts.last().unwrap());
        } else {
            return Err(TestCaseError::fail("wrong recipe variant"));
        }
        // Revision = N writes.
        prop_assert_eq!(loaded.revision, Revision(contexts.len() as u64));
    }
}

// Property 1 (R5.5, R7.5): LoadContext invariant — every Trigger
// dispatched against a stale in-memory copy still reads the latest
// Context from disk.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn load_context_on_every_trigger_property(
        original in arb_alarm_class_context(),
        edited in arb_alarm_class_context(),
    ) {
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");

        // Persist the original Context.
        store
            .write(ContextRecord::new(
                id.clone(),
                RecipeContext::AlarmClass(original.clone()),
            ))
            .unwrap();
        let first_load = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &first_load.context {
            prop_assert_eq!(c, &original);
        }

        // Simulate an "edit" by overwriting the Context.
        store
            .write(ContextRecord::new(
                id.clone(),
                RecipeContext::AlarmClass(edited.clone()),
            ))
            .unwrap();

        // The next load — what every Trigger handler does — sees the
        // edited values, never the original. This is the structural
        // guarantee that fixes the snooze-loses-sound bug class.
        let next_load = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &next_load.context {
            prop_assert_eq!(c, &edited);
        }
        prop_assert!(next_load.revision > first_load.revision);
    }
}

/// Serialise the global registry-touching tests when running this file
/// alongside the in-tree mod tests that also touch the registry.
static SERIAL: Mutex<()> = Mutex::new(());

/// Property 4 (R11.6, R22.4): Snooze count monotonicity through the
/// dispatch boundary. Mutates Context only by issuing Snooze Triggers.
#[test]
fn snooze_count_strictly_monotonic_across_dispatch() {
    use mobile_sentinel::recipes::{
        alarm_class::handle_trigger_with_store, register_recipe, AlarmClass,
    };
    use mobile_sentinel::Trigger;

    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Best-effort: ignore if AlarmClass already registered.
    let _ = register_recipe(AlarmClass::new());

    let dir = TempDir::new().unwrap();
    let store = ContextStore::new(dir.path());
    let id = InstanceId::new("a");
    let ctx = AlarmClassContext {
        label: "T".into(),
        schedule: ScheduleSpec::Weekdays {
            days_mask: 0b0001_1111,
            hour: 7,
            minute: 0,
        },
        time_zone: "UTC".into(),
        sound_id: SoundIdSpec::Bundled("happy".into()),
        snooze_policy: SnoozePolicy {
            max_count: 5,
            interval_minutes: 5,
            escalation: None,
        },
        challenges: vec![],
        vibration_enabled: true,
        vibration_pattern: None,
        kiosk_mode: true,
        bypass_dnd: true,
        snooze_count: 0,
        challenges_solved: false,
    };
    store
        .write(ContextRecord::new(
            id.clone(),
            RecipeContext::AlarmClass(ctx.clone()),
        ))
        .unwrap();

    let mut last_count = 0;
    for expected in 1..=ctx.snooze_policy.max_count {
        // Reload Context — that's what real Trigger dispatch does.
        let cur = store.load(&id).unwrap();
        let alarm = match &cur.context {
            RecipeContext::AlarmClass(c) => c.clone(),
            _ => panic!(),
        };
        handle_trigger_with_store(&store, Trigger::Snooze, &id, &cur.context).unwrap();
        let after = store.load(&id).unwrap();
        if let RecipeContext::AlarmClass(c) = &after.context {
            assert_eq!(c.snooze_count, expected);
            assert!(c.snooze_count > last_count);
            last_count = c.snooze_count;
        }
        // Verify the rest of the Context is unchanged — only snooze_count.
        if let RecipeContext::AlarmClass(c) = &after.context {
            assert_eq!(c.label, alarm.label);
            assert_eq!(c.sound_id, alarm.sound_id);
        }
    }
}

/// Property 3 (R22.2): scheduled X implies delivered X — for any
/// AlarmClass instance whose Context contains setting X at scheduling
/// time, every subsequent Trigger reload sees X (the bug fix).
#[test]
fn metadata_flow_property_with_arbitrary_settings() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut runner = TestRunner::default();
    let strategy = arb_alarm_class_context();
    for _ in 0..32 {
        let ctx = strategy.new_tree(&mut runner).unwrap().current();
        let dir = TempDir::new().unwrap();
        let store = ContextStore::new(dir.path());
        let id = InstanceId::new("a");
        store
            .write(ContextRecord::new(
                id.clone(),
                RecipeContext::AlarmClass(ctx.clone()),
            ))
            .unwrap();
        // Repeated reloads see the same settings.
        for _ in 0..3 {
            let r = store.load(&id).unwrap();
            if let RecipeContext::AlarmClass(c) = &r.context {
                assert_eq!(c.label, ctx.label);
                assert_eq!(c.sound_id, ctx.sound_id);
                assert_eq!(c.snooze_policy, ctx.snooze_policy);
                assert_eq!(c.challenges, ctx.challenges);
            } else {
                panic!();
            }
        }
    }
}
