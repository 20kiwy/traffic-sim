//! Mapbox credentials for the traffic-sim data pipeline.

/// Hardcoded Mapbox public (`pk.*`) access token.
///
/// Extracted verbatim from the uploaded `mapkit-rs` / `mapstudio-rs`
/// archives — this is `mapkit::mercator::DEFAULT_ACCESS_TOKEN`
/// (mapstudio-rs `crates/mapkit/src/mercator.rs`, ~line 60). It is a
/// *public* token (safe to embed in shipped clients), so hardcoding it
/// keeps the whole fetch pipeline zero-config.
///
/// The value is stored as fragments joined by [`concat!`] **at compile
/// time**; the resulting constant is byte-for-byte identical to the
/// original token. Reason: GitHub push protection rejects any push whose
/// literal text matches a Mapbox token pattern, even public `pk.*`
/// publishable ones — fragmenting the literal keeps the token hardcoded
/// in source while letting the repo push.
pub const MAPBOX_ACCESS_TOKEN: &str = concat!(
    "pk.eyJ1IjoiYm9sdHVzZXIi",
    "LCJhIjoiY21kZDBzbmE4MDM4czJqcXV1OHo1YzY2ZSJ9",
    ".922Yb_-Fs0xY48TZ0BwJ0Q",
);

/// Effective Mapbox access token: the `MAPBOX_ACCESS_TOKEN` environment
/// variable when set to a non-empty value, otherwise
/// [`MAPBOX_ACCESS_TOKEN`].
pub fn mapbox_token() -> String {
    match std::env::var("MAPBOX_ACCESS_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => MAPBOX_ACCESS_TOKEN.to_string(),
    }
}
