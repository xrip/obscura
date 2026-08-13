//! Fork-only: pick the wreq transport (TLS/H2) profile that matches the
//! selected fingerprint profile's Chrome major.
//!
//! Upstream pins one profile (Chrome145). The fork composes its browser
//! identity from a catalog that spans many Chrome majors, so the transport
//! has to follow, or the TLS fingerprint contradicts the UA on the wire.
//! Kept in its own file so upstream can keep editing wreq_client.rs freely.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};



pub(crate) const CHROME_TRANSPORT_PROFILES: &[(u32, wreq_util::Profile)] = &[
    (100, wreq_util::Profile::Chrome100),
    (101, wreq_util::Profile::Chrome101),
    (104, wreq_util::Profile::Chrome104),
    (105, wreq_util::Profile::Chrome105),
    (106, wreq_util::Profile::Chrome106),
    (107, wreq_util::Profile::Chrome107),
    (108, wreq_util::Profile::Chrome108),
    (109, wreq_util::Profile::Chrome109),
    (110, wreq_util::Profile::Chrome110),
    (114, wreq_util::Profile::Chrome114),
    (116, wreq_util::Profile::Chrome116),
    (117, wreq_util::Profile::Chrome117),
    (118, wreq_util::Profile::Chrome118),
    (119, wreq_util::Profile::Chrome119),
    (120, wreq_util::Profile::Chrome120),
    (123, wreq_util::Profile::Chrome123),
    (124, wreq_util::Profile::Chrome124),
    (126, wreq_util::Profile::Chrome126),
    (127, wreq_util::Profile::Chrome127),
    (128, wreq_util::Profile::Chrome128),
    (129, wreq_util::Profile::Chrome129),
    (130, wreq_util::Profile::Chrome130),
    (131, wreq_util::Profile::Chrome131),
    (132, wreq_util::Profile::Chrome132),
    (133, wreq_util::Profile::Chrome133),
    (134, wreq_util::Profile::Chrome134),
    (135, wreq_util::Profile::Chrome135),
    (136, wreq_util::Profile::Chrome136),
    (137, wreq_util::Profile::Chrome137),
    (138, wreq_util::Profile::Chrome138),
    (139, wreq_util::Profile::Chrome139),
    (140, wreq_util::Profile::Chrome140),
    (141, wreq_util::Profile::Chrome141),
    (142, wreq_util::Profile::Chrome142),
    (143, wreq_util::Profile::Chrome143),
    (144, wreq_util::Profile::Chrome144),
    (145, wreq_util::Profile::Chrome145),
    (146, wreq_util::Profile::Chrome146),
    (147, wreq_util::Profile::Chrome147),
    (148, wreq_util::Profile::Chrome148),
    (149, wreq_util::Profile::Chrome149),
];


/// Nearest transport profile to `browser_major`, and the major it belongs to.
/// The table is sparse because wreq only ships selected Chrome versions.
pub(crate) fn chrome_transport_profile(browser_major: u32) -> (u32, wreq_util::Profile) {
    CHROME_TRANSPORT_PROFILES
        .iter()
        .copied()
        .min_by_key(|(major, _)| major.abs_diff(browser_major))
        .expect("wreq Chrome transport profile table is not empty")
}


pub(crate) fn warn_transport_mismatch_once(browser_major: u32, transport_major: u32) {
    static WARNED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    let Ok(mut warned) = WARNED.get_or_init(|| Mutex::new(HashSet::new())).lock() else {
        return;
    };
    if warned.insert(browser_major) {
        tracing::warn!(
            browser_major,
            transport_major,
            "selected Chrome profile has no exact wreq transport; using the nearest transport profile"
        );
    }
}
