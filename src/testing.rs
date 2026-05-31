//! Test utilities for the universal SDK.
//!
//! The firing test double is [`MockFiringSink`] — an in-memory recorder
//! that implements [`crate::firing::FiringSink`]. Use it to assert
//! that Recipes and Kits issue the expected firing calls without a real
//! device.

/// In-memory [`FiringSink`](crate::firing::FiringSink) recorder for
/// testing Recipe + AlarmKit behaviour without a real platform backend.
/// External consumers writing tests against the SDK import it from here.
#[cfg(firing_enabled)]
pub use crate::firing::MockFiringSink;
