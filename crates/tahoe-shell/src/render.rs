// SPDX-License-Identifier: MIT OR Apache-2.0
//! The shell's renderer: a small software 2D rasteriser with real text.
//!
//! # Why software, and why here
//!
//! The shell's job is to draw a panel, a dock, windows and a launcher. That
//! is rounded rectangles, gradients, shadows and glyphs — none of which
//! needs a GPU, and all of which is easier to test when it is pure.
//!
//! Keeping it pure also means the same code produces the compositor's
//! chrome later: a `Canvas` is just pixels, and turning it into a
//! `wl_shm` buffer is a `memcpy` away. The shell's appearance is defined
//! once and rendered anywhere.
//!
//! # Text
//!
//! Glyphs come from `ab_glyph` (Apache-2.0), which parses the TrueType
//! files already on the system. Anti-aliasing is grayscale coverage, which
//! is what a non-subpixel-rendered compositor wants anyway: subpixel AA
//! assumes an RGB stripe order the compositor cannot know.
//!
//! Missing glyphs are drawn as a hollow box rather than silently omitted,
//! so a font problem is visible instead of producing a mysteriously short
//! string.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontArc, Glyph, PxScale, ScaleFont};

use crate::design::Rgba;

/// A rectangle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Rect {
    /// Build a rect.
    #[must_use]
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// True when the rect has a positive area.
    #[must_use]
    pub fn is_visible(self) -> bool {
        self.w > 0 && self.h > 0
    }

    /// Whether a point is inside, half-open on the right and bottom.
    #[must_use]
    pub fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }

    /// The rect moved by `(dx, dy)`.
    #[must_use]
    pub fn shifted(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    /// The overlapping area, if any.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }

    /// Shrink by `amount` on every side, never below zero.
    #[must_use]
    pub fn inset(self, amount: i32) -> Self {
        Self::new(
            self.x + amount,
            self.y + amount,
            (self.w - amount * 2).max(0),
            (self.h - amount * 2).max(0),
        )
    }
}

/// Where a loaded font came from, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSource {
    /// Path the font was read from.
    pub path: PathBuf,
    /// Family name as the file reports it.
    pub family: Option<String>,
}

/// Fonts the shell needs.
///
/// Two weights only. A desktop that needs more is a desktop that has not
/// decided what it looks like.
pub struct Fonts {
    /// Body text.
    pub regular: FontArc,
    /// Headings and titles.
    pub bold: FontArc,
    /// Where they came from.
    pub sources: Vec<FontSource>,
}

/// Candidate font files, in preference order.
///
/// All of these ship with common Linux distributions. The list is a
/// fallback chain rather than a dependency: a shell that refuses to start
/// without a specific font is worse than one that renders in a different
/// typeface.
const REGULAR_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/TTF/LiberationSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/NotoSans-Regular.ttf",
];

/// Bold candidates.
const BOLD_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/TTF/LiberationSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/TTF/NotoSans-Bold.ttf",
];

impl Fonts {
    /// Load from the system, preferring the paths in [`REGULAR_CANDIDATES`].
    ///
    /// # Errors
    ///
    /// Returns an error only when *no* usable font exists, which means the
    /// machine has no fonts at all and nothing legible can be drawn.
    pub fn system() -> Result<Self, String> {
        let (regular, regular_source) = load_first(REGULAR_CANDIDATES, "regular")?;
        // A missing bold is not fatal: the regular face is reused. A desktop
        // that refused to start because a bold font was absent would be
        // worse than one that renders in two weights of the same face.
        let mut sources = vec![regular_source];
        let bold = match load_first(BOLD_CANDIDATES, "bold") {
            Ok((bold, source)) => {
                sources.push(source);
                bold
            }
            Err(_) => regular.clone(),
        };
        Ok(Self {
            regular,
            bold,
            sources,
        })
    }

    /// Load an explicit pair, for tests and for a caller that ships fonts.
    ///
    /// # Errors
    ///
    /// Returns an error when either file cannot be read or parsed.
    pub fn from_files(regular: &Path, bold: &Path) -> Result<Self, String> {
        let read = |p: &Path| -> Result<FontArc, String> {
            let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
            FontArc::try_from_vec(bytes).map_err(|e| format!("{}: {e}", p.display()))
        };
        Ok(Self {
            regular: read(regular)?,
            bold: read(bold)?,
            sources: vec![
                FontSource {
                    path: regular.to_path_buf(),
                    family: None,
                },
                FontSource {
                    path: bold.to_path_buf(),
                    family: None,
                },
            ],
        })
    }
}

fn load_first(candidates: &[&str], weight: &str) -> Result<(FontArc, FontSource), String> {
    let mut tried = Vec::new();
    for path in candidates {
        let path = Path::new(path);
        if !path.is_file() {
            tried.push(format!("{} (absent)", path.display()));
            continue;
        }
        match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| FontArc::try_from_vec(bytes).map_err(|e| e.to_string()))
        {
            Ok(font) => {
                return Ok((
                    font,
                    FontSource {
                        path: path.to_path_buf(),
                        family: None,
                    },
                ));
            }
            Err(e) => tried.push(format!("{} ({e})", path.display())),
        }
    }
    Err(format!(
        "no {weight} font found; tried: {}",
        tried.join(", ")
    ))
}

/// Which face to draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Body text.
    Regular,
    /// Headings and titles.
    Bold,
}

/// How to align text horizontally within its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Left edge at the box origin.
    Left,
    /// Centred in the box.
    Center,
    /// Right edge at the box's right edge.
    Right,
}

/// An RGBA8 pixel buffer with drawing operations.
///
/// Row-major, top row first, straight (non-premultiplied) alpha — the same
/// convention as the design system, so nothing converts anywhere.
pub struct Canvas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    fonts: Option<Fonts>,
}

impl Canvas {
    /// A transparent-black canvas.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
            fonts: None,
        }
    }

    /// Attach fonts so text can be drawn.
    #[must_use]
    pub fn with_fonts(mut self, fonts: Fonts) -> Self {
        self.fonts = Some(fonts);
        self
    }

    /// The fonts, when attached.
    #[must_use]
    pub fn fonts(&self) -> Option<&Fonts> {
        self.fonts.as_ref()
    }

    /// The whole canvas as a rect.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// Read a pixel as RGBA, or `None` outside the canvas.
    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some([
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ])
    }

    /// Blend a colour over a pixel at full destination coverage.
    pub fn blend(&mut self, x: i32, y: i32, colour: Rgba, coverage: f32) {
        if coverage <= 0.0 || x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let alpha = (colour.a * coverage).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let dst = [
            f32::from(self.pixels[i]) / 255.0,
            f32::from(self.pixels[i + 1]) / 255.0,
            f32::from(self.pixels[i + 2]) / 255.0,
            f32::from(self.pixels[i + 3]) / 255.0,
        ];
        let src = [colour.r, colour.g, colour.b];
        // Source-over in straight alpha: the destination keeps its alpha and
        // gains the source's contribution.
        let out_a = alpha + dst[3] * (1.0 - alpha);
        for channel in 0..3 {
            let value = if out_a <= f32::EPSILON {
                src[channel]
            } else {
                (src[channel] * alpha + dst[channel] * dst[3] * (1.0 - alpha)) / out_a
            };
            self.pixels[i + channel] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        self.pixels[i + 3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
    }

    /// Fill a rectangle.
    pub fn fill_rect(&mut self, rect: Rect, colour: Rgba) {
        if !rect.is_visible() {
            return;
        }
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                self.blend(x, y, colour, 1.0);
            }
        }
    }

    /// Fill a rectangle with a vertical gradient sampled from `sample`.
    ///
    /// `sample` takes a value in `0..=1` from the top of the rect to the
    /// bottom, which is exactly the shape [`crate::design::DesignSystem::wallpaper_at`]
    /// has — so the wallpaper is drawn by the same function the compositor
    /// uploads as a texture.
    pub fn fill_gradient(&mut self, rect: Rect, sample: impl Fn(f32) -> Rgba) {
        if !rect.is_visible() {
            return;
        }
        let span = (rect.h - 1).max(1) as f32;
        for y in rect.y..rect.y + rect.h {
            let t = (y - rect.y) as f32 / span;
            let colour = sample(t);
            for x in rect.x..rect.x + rect.w {
                self.blend(x, y, colour, 1.0);
            }
        }
    }

    /// Fill a rounded rectangle with anti-aliased corners.
    ///
    /// Coverage is computed from the distance to the corner centre, which
    /// gives one pixel of softness — enough that a 12-pixel radius does not
    /// look like a staircase, without a full signed-distance-field
    /// rasteriser.
    pub fn fill_rounded(&mut self, rect: Rect, radius: i32, colour: Rgba) {
        if !rect.is_visible() {
            return;
        }
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        if radius == 0 {
            self.fill_rect(rect, colour);
            return;
        }
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                self.blend(x, y, colour, rounded_coverage(rect, radius, x, y));
            }
        }
    }

    /// Draw a 1-pixel outline following [`Canvas::fill_rounded`]'s shape.
    ///
    /// Outer coverage minus inner coverage is exactly a one-pixel ring, and
    /// computing it directly avoids the clone-and-mask approach that was
    /// here first and got the corner alpha wrong.
    pub fn stroke_rounded(&mut self, rect: Rect, radius: i32, colour: Rgba) {
        if !rect.is_visible() {
            return;
        }
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0);
        let inner = Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
        let inner_radius = radius.saturating_sub(1);
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                let outer_cov = rounded_coverage(rect, radius, x, y);
                if outer_cov <= 0.0 {
                    continue;
                }
                let inner_cov = if inner.is_visible() {
                    rounded_coverage(inner, inner_radius, x, y)
                } else {
                    0.0
                };
                self.blend(x, y, colour, (outer_cov - inner_cov).clamp(0.0, 1.0));
            }
        }
    }

    /// Fill a rectangle with an opaque colour, ignoring what is underneath.
    pub fn clear_rect(&mut self, rect: Rect, colour: Rgba) {
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
                    continue;
                }
                let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
                let a = colour.a.clamp(0.0, 1.0);
                self.pixels[i] = (colour.r * 255.0).round() as u8;
                self.pixels[i + 1] = (colour.g * 255.0).round() as u8;
                self.pixels[i + 2] = (colour.b * 255.0).round() as u8;
                self.pixels[i + 3] = (a * 255.0).round() as u8;
            }
        }
    }

    /// A soft drop shadow behind a rounded rect.
    ///
    /// Drawn as concentric rounded rects of decreasing alpha rather than a
    /// blur: it costs no extra memory and looks the same at these radii.
    pub fn drop_shadow(&mut self, rect: Rect, radius: i32, spread: i32, strength: f32) {
        for step in (1..=spread).rev() {
            let alpha = strength * (1.0 - step as f32 / (spread as f32 + 1.0)) * 0.10;
            self.fill_rounded(
                Rect::new(
                    rect.x - step,
                    rect.y - step + spread / 3,
                    rect.w + step * 2,
                    rect.h + step * 2,
                ),
                radius + step,
                Rgba::new(0.0, 0.0, 0.0, alpha),
            );
        }
    }

    /// The rendered size of a string, in pixels.
    #[must_use]
    pub fn measure(&self, text: &str, size: f32, face: Face) -> (i32, i32) {
        let Some(fonts) = &self.fonts else {
            return (0, 0);
        };
        let font = match face {
            Face::Regular => &fonts.regular,
            Face::Bold => &fonts.bold,
        };
        let scaled = font.as_scaled(PxScale::from(size));
        let width: f32 = text
            .chars()
            .map(|c| {
                let glyph = scaled.scaled_glyph(c);
                scaled.h_advance(glyph.id)
            })
            .sum();
        let ascent = scaled.ascent();
        let descent = scaled.descent();
        (width.ceil() as i32, (ascent - descent).ceil() as i32)
    }

    /// Draw a string.
    ///
    /// `box_rect` positions the text: `align` decides horizontal placement
    /// and the string is vertically centred on `baseline_y` when given, or
    /// on the box otherwise.
    pub fn draw_text(
        &mut self,
        text: &str,
        rect: Rect,
        size: f32,
        face: Face,
        colour: Rgba,
        align: Align,
    ) {
        let Some(fonts) = &self.fonts else {
            return;
        };
        let font = match face {
            Face::Regular => fonts.regular.clone(),
            Face::Bold => fonts.bold.clone(),
        };
        let scaled = font.as_scaled(PxScale::from(size));
        let (width, _) = self.measure(text, size, face);
        let start_x = match align {
            Align::Left => rect.x,
            Align::Center => rect.x + (rect.w - width) / 2,
            Align::Right => rect.x + rect.w - width,
        };
        // `ascent` is positive above the baseline and `descent` negative
        // below it, so the visual height is the difference. Centring on that
        // is what makes text sit in the middle of a control rather than low.
        let ascent = scaled.ascent();
        let descent = scaled.descent();
        let baseline = rect.y as f32 + (rect.h as f32 - (ascent - descent)) / 2.0 + ascent;
        let baseline = baseline.round() as i32;
        let mut pen_x = start_x as f32;
        for c in text.chars() {
            let glyph_id = scaled.scaled_glyph(c).id;
            let advance = scaled.h_advance(glyph_id);
            let glyph: Glyph = scaled.scaled_glyph(c);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                // `draw` yields coordinates inside the outline's own
                // bounds, so both the bounds origin and the pen position are
                // added.
                outlined.draw(|gx, gy, coverage| {
                    let px = pen_x.round() as i32 + bounds.min.x as i32 + gx as i32;
                    let py = baseline + bounds.min.y as i32 + gy as i32;
                    self.blend(px, py, colour, coverage);
                });
            } else if !c.is_whitespace() {
                // A missing glyph is drawn as a hollow box, so a font
                // problem is visible rather than a mysteriously short word.
                let size_i = size as i32;
                let box_rect = Rect::new(pen_x.round() as i32, baseline - size_i, size_i, size_i);
                self.stroke_rounded(box_rect, 1, colour.with_alpha(0.55));
            }
            pen_x += advance;
        }
    }

    /// Encode as a PNG.
    ///
    /// Uses stored (uncompressed) deflate blocks, which are valid zlib and
    /// need no dependency. The files are large; a shipping shell would use a
    /// real encoder, and the size is the only reason to.
    #[must_use]
    pub fn to_png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity((self.height as usize) * (self.width as usize * 4 + 1));
        for y in 0..self.height as usize {
            // Filter type 0 (None) per scanline.
            raw.push(0);
            let start = y * self.width as usize * 4;
            raw.extend_from_slice(&self.pixels[start..start + self.width as usize * 4]);
        }

        let mut zlib = vec![0x78, 0x01];
        // Deflate stored blocks: 5-byte header per block, 65535 bytes max.
        for (i, chunk) in raw.chunks(65_535).enumerate() {
            let last = (i + 1) * 65_535 >= raw.len();
            zlib.push(if last { 1 } else { 0 });
            let len = chunk.len() as u16;
            zlib.extend_from_slice(&len.to_le_bytes());
            zlib.extend_from_slice(&(!len).to_le_bytes());
            zlib.extend_from_slice(chunk);
        }
        // Adler-32 of the uncompressed data.
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &raw {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
        write_chunk(&mut png, b"IHDR", &ihdr);
        write_chunk(&mut png, b"IDAT", &zlib);
        write_chunk(&mut png, b"IEND", &[]);
        png
    }
}

/// Anti-aliased coverage of a rounded rectangle at a pixel centre.
fn rounded_coverage(rect: Rect, radius: i32, x: i32, y: i32) -> f32 {
    if !rect.contains(x, y) {
        return 0.0;
    }
    if radius == 0 {
        return 1.0;
    }
    let r = radius as f32;
    let dx = if x < rect.x + radius {
        (rect.x + radius - x) as f32
    } else if x >= rect.x + rect.w - radius {
        (x - (rect.x + rect.w - radius - 1)) as f32
    } else {
        0.0
    };
    let dy = if y < rect.y + radius {
        (rect.y + radius - y) as f32
    } else if y >= rect.y + rect.h - radius {
        (y - (rect.y + rect.h - radius - 1)) as f32
    } else {
        0.0
    };
    if dx == 0.0 || dy == 0.0 {
        return 1.0;
    }
    let distance = (dx * dx + dy * dy).sqrt();
    (r + 0.5 - distance).clamp(0.0, 1.0)
}

/// One 4-byte PNG length, 4-byte tag, body, 4-byte CRC. The only reason
/// this function exists is that PNG's chunk framing is easy to get subtly
/// wrong and impossible to debug by eye.
fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    let crc = crc32(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// CRC-32 as PNG specifies it.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests;
