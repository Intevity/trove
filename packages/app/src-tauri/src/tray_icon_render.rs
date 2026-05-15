//! Threshold-tinted variants of the tray icon, cached after first render.
//!
//! The source PNG (`icons/tray-icon.png`) is a 32x32 silhouette with an
//! alpha channel. For each tint we replace the RGB of every non-
//! transparent pixel with the tint color while preserving the original
//! alpha — antialiased edges stay smooth, the silhouette stays sharp.
//!
//! Mirrors `/Users/jeff/github/claude-sentinel/packages/app/src-tauri/
//! src/tray_icon_render.rs`. Trove's palette is green/amber/red instead
//! of the four-state ios palette claude-sentinel uses; the technique is
//! identical.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray-icon.png");

/// Tray-icon tint matching the Tailwind `health` palette in
/// `packages/app/tailwind.config.js`, plus the Trove `brand` token
/// (teal) used in place of `Green` for the healthy tray state. The
/// mapping from `OverallHealth → TintColor` lives in
/// `tray.rs::derive_tint_color`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TintColor {
    /// Healthy — collector running, recent telemetry observed. Mirrors
    /// the Tailwind `health.green` token; kept available for tests and
    /// for future surfaces that want the system-green tint, but the
    /// tray pipeline routes healthy → `Brand` (teal) instead.
    #[allow(dead_code)]
    Green,
    /// Warning — running but transitional, no recent traffic, or
    /// metrics endpoint unreachable.
    Amber,
    /// Sidecar crashed, failed to spawn, or otherwise dead.
    Red,
    /// Healthy — Trove brand teal; mirrors the `brand` Tailwind token.
    /// Replaces `Green` for the live tray retint so the menu-bar icon
    /// carries the product brand at rest while still flipping to amber
    /// or red when something is wrong.
    Brand,
}

impl TintColor {
    /// `(R, G, B)` — `Green/Amber/Red` match `tailwind.config.js`
    /// `health.{green,amber,red}`; `Brand` matches the Tailwind
    /// `brand` token.
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            TintColor::Green => (0x10, 0xB9, 0x81),
            TintColor::Amber => (0xF5, 0x9E, 0x0B),
            TintColor::Red => (0xEF, 0x44, 0x44),
            TintColor::Brand => (0x2D, 0xBF, 0xB8),
        }
    }
}

/// Raw RGBA buffer ready to hand to `tauri::image::Image::new`.
#[derive(Debug)]
pub struct RgbaBuffer {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

static CACHE: OnceLock<std::sync::Mutex<HashMap<TintColor, Arc<RgbaBuffer>>>> = OnceLock::new();

fn cache() -> &'static std::sync::Mutex<HashMap<TintColor, Arc<RgbaBuffer>>> {
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Returns the tinted icon for `color`, decoding+tinting once and
/// caching. Panics only if the embedded PNG fails to decode — which
/// would be a build-time regression, not a runtime condition.
#[must_use]
pub fn tinted(color: TintColor) -> Arc<RgbaBuffer> {
    {
        let map = cache().lock().expect("tinted-cache mutex poisoned");
        if let Some(buf) = map.get(&color) {
            return buf.clone();
        }
    }

    let img = image::load_from_memory(TRAY_ICON_PNG)
        .expect("embedded tray-icon.png failed to decode")
        .to_rgba8();
    let (width, height) = img.dimensions();
    let (r, g, b) = color.rgb();
    let mut bytes = img.into_raw();
    for px in bytes.chunks_exact_mut(4) {
        if px[3] != 0 {
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
    }

    let buf = Arc::new(RgbaBuffer {
        bytes,
        width,
        height,
    });
    let mut map = cache().lock().expect("tinted-cache mutex poisoned");
    map.entry(color).or_insert_with(|| buf.clone()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tint_returns_buffer_with_expected_color() {
        for color in [
            TintColor::Green,
            TintColor::Amber,
            TintColor::Red,
            TintColor::Brand,
        ] {
            let buf = tinted(color);
            assert_eq!(buf.bytes.len(), (buf.width * buf.height * 4) as usize);
            let (r, g, b) = color.rgb();
            // Find at least one fully-opaque pixel and verify it carries the tint.
            let opaque = buf
                .bytes
                .chunks_exact(4)
                .find(|px| px[3] == 0xFF)
                .expect("tray icon has no fully opaque pixels");
            assert_eq!((opaque[0], opaque[1], opaque[2]), (r, g, b));
        }
    }

    #[test]
    fn transparent_pixels_have_alpha_zero() {
        let buf = tinted(TintColor::Red);
        // Any fully-transparent pixel should remain alpha == 0. We
        // don't assert RGB because the source PNG carries arbitrary
        // values in transparent regions; the contract is alpha only.
        let transparent_count = buf.bytes.chunks_exact(4).filter(|px| px[3] == 0).count();
        assert!(
            transparent_count > 0,
            "tray-icon.png has no fully-transparent pixels — test premise broken"
        );
    }

    #[test]
    fn cache_returns_same_arc_on_second_call() {
        let a = tinted(TintColor::Amber);
        let b = tinted(TintColor::Amber);
        assert!(
            Arc::ptr_eq(&a, &b),
            "expected cached Arc to be reused, got two distinct allocations"
        );
    }

    #[test]
    fn rgb_values_match_tailwind_palette() {
        // Lockstep with `packages/app/tailwind.config.js`. Green/Amber/
        // Red mirror the `health.*` tokens; Brand mirrors the `brand`
        // token used by the in-app rebrand surfaces. If any hex value
        // changes this test fails on purpose so the tray and the
        // dashboard surfaces stay visually consistent.
        assert_eq!(TintColor::Green.rgb(), (0x10, 0xB9, 0x81));
        assert_eq!(TintColor::Amber.rgb(), (0xF5, 0x9E, 0x0B));
        assert_eq!(TintColor::Red.rgb(), (0xEF, 0x44, 0x44));
        assert_eq!(TintColor::Brand.rgb(), (0x2D, 0xBF, 0xB8));
    }
}
