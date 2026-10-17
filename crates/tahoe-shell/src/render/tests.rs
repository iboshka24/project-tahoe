// SPDX-License-Identifier: MIT OR Apache-2.0
//! Renderer tests.
//!
//! These assert geometry and pixel math, not taste. The one taste judgement
//! is that text is legible, which is checked by counting ink rather than by
//! looking.

use super::*;

fn canvas_with_fonts(w: u32, h: u32) -> Canvas {
    match Fonts::system() {
        Ok(fonts) => Canvas::new(w, h).with_fonts(fonts),
        Err(e) => panic!("no system font available: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Rect
// ---------------------------------------------------------------------------

#[test]
fn rect_contains_is_half_open() {
    let r = Rect::new(10, 10, 100, 50);
    assert!(r.contains(10, 10));
    assert!(r.contains(109, 59));
    assert!(!r.contains(110, 59), "right edge excluded");
    assert!(!r.contains(109, 60), "bottom edge excluded");
}

#[test]
fn rect_intersection_and_inset() {
    let a = Rect::new(0, 0, 10, 10);
    assert_eq!(
        a.intersection(Rect::new(5, 5, 10, 10)),
        Some(Rect::new(5, 5, 5, 5))
    );
    assert_eq!(a.intersection(Rect::new(10, 0, 5, 5)), None);
    assert_eq!(a.inset(2), Rect::new(2, 2, 6, 6));
    // Inset past the middle must not produce a negative size.
    assert_eq!(a.inset(20), Rect::new(20, 20, 0, 0));
    assert!(!a.inset(20).is_visible());
}

// ---------------------------------------------------------------------------
// Pixel operations
// ---------------------------------------------------------------------------

#[test]
fn a_new_canvas_is_transparent() {
    let c = Canvas::new(4, 4);
    assert_eq!(c.get(0, 0), Some([0, 0, 0, 0]));
    assert_eq!(c.get(4, 4), None);
}

#[test]
fn an_opaque_fill_replaces_the_pixel() {
    let mut c = Canvas::new(2, 2);
    c.fill_rect(Rect::new(0, 0, 1, 1), Rgba::rgb8(10, 20, 30));
    assert_eq!(c.get(0, 0), Some([10, 20, 30, 255]));
    assert_eq!(
        c.get(1, 0),
        Some([0, 0, 0, 0]),
        "the fill must stay inside its rect"
    );
}

#[test]
fn blending_is_source_over() {
    let mut c = Canvas::new(1, 1);
    c.blend(0, 0, Rgba::rgb8(255, 255, 255), 1.0);
    c.blend(0, 0, Rgba::rgb8(0, 0, 0).with_alpha(0.5), 1.0);
    let [r, _, _, a] = c.get(0, 0).expect("pixel");
    assert!((120..=135).contains(&r), "half black over white gave {r}");
    assert_eq!(a, 255, "an opaque destination stays opaque");
}

#[test]
fn zero_coverage_draws_nothing() {
    let mut c = Canvas::new(1, 1);
    c.blend(0, 0, Rgba::rgb8(255, 0, 0), 0.0);
    assert_eq!(c.get(0, 0), Some([0, 0, 0, 0]));
}

#[test]
fn drawing_out_of_bounds_is_ignored() {
    let mut c = Canvas::new(2, 2);
    c.blend(-5, 0, Rgba::rgb8(255, 0, 0), 1.0);
    c.blend(0, -5, Rgba::rgb8(255, 0, 0), 1.0);
    c.blend(99, 0, Rgba::rgb8(255, 0, 0), 1.0);
    c.fill_rect(Rect::new(-10, -10, 100, 100), Rgba::rgb8(0, 255, 0));
    // The in-bounds part was drawn; nothing panicked.
    assert_eq!(c.get(0, 0), Some([0, 255, 0, 255]));
}

#[test]
fn a_gradient_varies_down_the_rect_and_is_flat_across_it() {
    let mut c = Canvas::new(8, 32);
    c.fill_gradient(Rect::new(0, 0, 8, 32), |t| Rgba::new(t, 0.0, 0.0, 1.0));
    let top = c.get(4, 0).expect("top");
    let bottom = c.get(4, 31).expect("bottom");
    assert!(top[0] < 8, "top should be near black, got {top:?}");
    assert!(
        bottom[0] > 247,
        "bottom should be near white, got {bottom:?}"
    );
    assert_eq!(
        c.get(0, 16),
        c.get(7, 16),
        "a vertical gradient is constant across x"
    );
}

#[test]
fn rounded_corners_are_transparent_and_the_middle_is_not() {
    let mut c = Canvas::new(40, 40);
    c.fill_rounded(Rect::new(0, 0, 40, 40), 12, Rgba::rgb8(255, 255, 255));
    assert_eq!(
        c.get(20, 20),
        Some([255, 255, 255, 255]),
        "the centre is filled"
    );
    assert_eq!(c.get(0, 0), Some([0, 0, 0, 0]), "the corner is not");
    // The edge midpoint stays inside.
    assert_eq!(c.get(20, 0), Some([255, 255, 255, 255]));
    assert_eq!(c.get(0, 20), Some([255, 255, 255, 255]));
}

#[test]
fn a_zero_radius_rounded_rect_is_a_plain_rect() {
    let mut c = Canvas::new(10, 10);
    c.fill_rounded(Rect::new(0, 0, 10, 10), 0, Rgba::rgb8(255, 255, 255));
    assert_eq!(c.get(0, 0), Some([255, 255, 255, 255]));
}

#[test]
fn a_radius_larger_than_the_rect_is_clamped() {
    // Must not produce a negative radius or an empty shape.
    let mut c = Canvas::new(10, 10);
    c.fill_rounded(Rect::new(0, 0, 10, 10), 500, Rgba::rgb8(255, 255, 255));
    assert_eq!(c.get(5, 5), Some([255, 255, 255, 255]));
}

#[test]
fn an_outline_leaves_the_interior_untouched() {
    let mut c = Canvas::new(30, 30);
    c.fill_rounded(Rect::new(0, 0, 30, 30), 8, Rgba::rgb8(0, 0, 0));
    c.stroke_rounded(Rect::new(5, 5, 20, 20), 6, Rgba::rgb8(255, 0, 0));
    let border = c.get(15, 5).expect("top edge");
    assert!(border[0] > 200, "the outline should be red, got {border:?}");
    // One pixel inside the outline is still the black fill.
    let inside = c.get(15, 8).expect("interior");
    assert!(
        inside[0] < 40,
        "the interior must be untouched, got {inside:?}"
    );
}

#[test]
fn a_shadow_darkens_without_covering() {
    let mut c = Canvas::new(60, 60);
    c.fill_rect(Rect::new(0, 0, 60, 60), Rgba::rgb8(255, 255, 255));
    c.drop_shadow(Rect::new(20, 20, 20, 20), 6, 10, 1.0);
    let near = c.get(18, 25).expect("near the shadow");
    let far = c.get(1, 1).expect("far from it");
    assert!(
        near[0] < far[0],
        "closer to the shape should be darker: {near:?} vs {far:?}"
    );
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

#[test]
fn measuring_a_string_is_monotonic_in_its_length() {
    let c = canvas_with_fonts(100, 100);
    let (a, _) = c.measure("a", 14.0, Face::Regular);
    let (abc, _) = c.measure("abc", 14.0, Face::Regular);
    let (long, _) = c.measure("a much longer string", 14.0, Face::Regular);
    assert!(a > 0, "a glyph has width");
    assert!(abc > a, "{abc} should exceed {a}");
    assert!(long > abc);
}

#[test]
fn measuring_is_monotonic_in_the_size() {
    let c = canvas_with_fonts(100, 100);
    let (small, _) = c.measure("Tahoe", 10.0, Face::Regular);
    let (large, _) = c.measure("Tahoe", 20.0, Face::Regular);
    assert!(
        large > small * 15 / 10,
        "{large} should be about twice {small}"
    );
}

#[test]
fn an_empty_string_has_no_width_but_still_has_a_line_height() {
    // The width is zero; the height is the font's line box, because "where
    // would the next line go" is still a meaningful question for "".
    let c = canvas_with_fonts(100, 100);
    let (w, h) = c.measure("", 14.0, Face::Regular);
    assert_eq!(w, 0);
    assert!(h > 0, "an empty line still has a height, got {h}");
}

#[test]
fn drawing_text_puts_ink_on_the_canvas() {
    // Legibility is not a pixel assertion, but "did anything get drawn" is,
    // and a font path that silently fails is the realistic failure.
    let mut c = canvas_with_fonts(200, 40);
    c.fill_rect(Rect::new(0, 0, 200, 40), Rgba::rgb8(255, 255, 255));
    c.draw_text(
        "Tahoe",
        Rect::new(6, 0, 180, 40),
        18.0,
        Face::Bold,
        Rgba::rgb8(0, 0, 0),
        Align::Left,
    );
    let ink = (0..200)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .filter(|(x, y)| c.get(*x, *y).is_some_and(|p| p[0] < 128))
        .count();
    assert!(
        ink > 100,
        "expected visible glyphs, found {ink} dark pixels"
    );
}

#[test]
fn alignment_positions_the_string() {
    let mut c = canvas_with_fonts(300, 40);
    c.fill_rect(Rect::new(0, 0, 300, 40), Rgba::rgb8(255, 255, 255));
    let box_rect = Rect::new(0, 0, 300, 40);
    let ink_bounds = |c: &Canvas| -> (i32, i32) {
        let mut min = i32::MAX;
        let mut max = i32::MIN;
        for x in 0..300 {
            for y in 0..40 {
                if c.get(x, y).is_some_and(|p| p[0] < 128) {
                    min = min.min(x);
                    max = max.max(x);
                }
            }
        }
        (min, max)
    };
    c.draw_text(
        "centre",
        box_rect,
        16.0,
        Face::Regular,
        Rgba::rgb8(0, 0, 0),
        Align::Center,
    );
    let (min, max) = ink_bounds(&c);
    let centre = (min + max) / 2;
    assert!(
        (centre - 150).abs() < 12,
        "centred text should sit near 150, got {centre}"
    );

    let mut left = canvas_with_fonts(300, 40);
    left.fill_rect(Rect::new(0, 0, 300, 40), Rgba::rgb8(255, 255, 255));
    left.draw_text(
        "centre",
        box_rect,
        16.0,
        Face::Regular,
        Rgba::rgb8(0, 0, 0),
        Align::Left,
    );
    let (min, _) = ink_bounds(&left);
    assert!(
        min < centre,
        "left-aligned text should start further left, got {min}"
    );
}

#[test]
fn text_without_fonts_is_silently_skipped_rather_than_panicking() {
    // A Canvas with no fonts is a legal state — the preview renderer uses
    // one — and drawing text into it must be a no-op, not a panic.
    let mut c = Canvas::new(40, 20);
    c.draw_text(
        "nope",
        Rect::new(0, 0, 40, 20),
        12.0,
        Face::Regular,
        Rgba::rgb8(255, 0, 0),
        Align::Left,
    );
    assert_eq!(c.measure("nope", 12.0, Face::Regular), (0, 0));
    assert_eq!(c.get(0, 0), Some([0, 0, 0, 0]));
}

#[test]
fn the_font_chain_reports_where_it_loaded_from() {
    let fonts = Fonts::system().expect("a system font");
    assert!(
        !fonts.sources.is_empty(),
        "the source path must be reportable"
    );
    for source in &fonts.sources {
        assert!(
            source.path.is_file(),
            "{} should exist",
            source.path.display()
        );
    }
}

#[test]
fn a_missing_font_file_is_a_named_error() {
    match Fonts::from_files(Path::new("/nope.ttf"), Path::new("/nope-bold.ttf")) {
        Err(message) => assert!(message.contains("/nope.ttf"), "{message}"),
        Ok(_) => panic!("a missing font must not load"),
    }
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

#[test]
fn a_png_has_the_right_framing() {
    let c = Canvas::new(4, 3);
    let png = c.to_png();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "the PNG signature");
    // IHDR length and tag.
    assert_eq!(&png[8..12], &13u32.to_be_bytes());
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(&png[16..20], &4u32.to_be_bytes(), "width");
    assert_eq!(&png[20..24], &3u32.to_be_bytes(), "height");
    assert_eq!(&png[24..29], &[8, 6, 0, 0, 0], "8-bit RGBA");
    assert!(png.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
}

#[test]
fn png_chunk_crcs_are_correct() {
    // A wrong CRC makes the file unreadable and is invisible to the eye, so
    // it is worth recomputing here rather than trusting.
    let c = Canvas::new(2, 2);
    let png = c.to_png();
    let mut at = 8usize;
    let mut seen = Vec::new();
    while at + 12 <= png.len() {
        let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
        let tag = &png[at + 4..at + 8];
        let body_start = at + 8;
        let crc_at = body_start + len;
        let expected = u32::from_be_bytes([
            png[crc_at],
            png[crc_at + 1],
            png[crc_at + 2],
            png[crc_at + 3],
        ]);
        assert_eq!(
            crc32(&png[at + 4..crc_at]),
            expected,
            "CRC of {:?}",
            String::from_utf8_lossy(tag)
        );
        seen.push(String::from_utf8_lossy(tag).into_owned());
        at = crc_at + 4;
    }
    assert_eq!(seen, vec!["IHDR", "IDAT", "IEND"]);
    assert_eq!(at, png.len(), "the chunks must cover the file exactly");
}

#[test]
fn a_large_canvas_produces_multiple_deflate_blocks() {
    // The stored-block header is 5 bytes and holds a 16-bit length, so this
    // is the boundary a hand-written encoder gets wrong.
    let c = Canvas::new(200, 200);
    let png = c.to_png();
    assert!(
        png.len() > 200 * 200 * 4,
        "stored blocks are not compressed"
    );
    // Still well-framed.
    assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]));
}

#[test]
fn the_zlib_adler_checksum_is_present() {
    let c = Canvas::new(3, 3);
    let png = c.to_png();
    // Find IDAT and check the trailing four bytes are an Adler-32 of
    // something rather than zero, which is the value a missing computation
    // would leave.
    let at = png
        .windows(4)
        .position(|w| w == b"IDAT")
        .expect("an IDAT chunk");
    let len = u32::from_be_bytes([png[at - 4], png[at - 3], png[at - 2], png[at - 1]]) as usize;
    let body = &png[at + 4..at + 4 + len];
    let adler = u32::from_be_bytes([body[len - 4], body[len - 3], body[len - 2], body[len - 1]]);
    assert_ne!(adler, 0, "the Adler-32 must actually be computed");
}
