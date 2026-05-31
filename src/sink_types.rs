//! Shared data types for the feature-gated capability modules.
//!
//! These are the structured values that capability functions accept and
//! return (permissions, biometrics, network, location, contacts, calendar).
//! Consolidating them here keeps the capability modules thin and avoids each
//! re-declaring its own value types.

use std::time::SystemTime;

/// Runtime permission status. Returned by [`crate::permissions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
}

/// Biometric hardware type available on the device. Returned by
/// [`crate::biometric`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometricType {
    Face,
    Fingerprint,
    None,
}

/// Network connection type. Returned by [`crate::network`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    Wifi,
    Cellular,
    None,
}

/// A geographic coordinate. Used by [`crate::location`] and [`crate::maps`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
}

/// A device contact entry. Returned by [`crate::contacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: String,
    pub display_name: String,
    pub phone_numbers: Vec<String>,
    pub email_addresses: Vec<String>,
}

/// A calendar event. Used by [`crate::calendar`].
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_time: SystemTime,
    pub end_time: SystemTime,
    pub location: Option<String>,
}
