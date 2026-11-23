// SPDX-License-Identifier: GPL-3.0-or-later

//! A software renderer for the engine's draw list. It draws one window into one buffer of
//! pixels, one device pixel per layout pixel: the app's layer makes the window larger by whole
//! numbers. A pixel is `0xAARRGGBB` with its color multiplied by its alpha, which in memory is
//! the BGRA an `IOSurface` holds. Where the skin draws nothing the buffer stays clear, so a
//! shaped window shows the desktop around its shape.

use std::rc::Rc;

use walkit::draw::{DrawCmd, Gamma, Rect, Region, TextCmd};
use walkit::truetype::{em, Style};

use crate::host::Assets;

/// A window's pixels: `w` x `h`, rows top first.
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        let (w, h) = (w.max(0), h.max(0));
        Canvas {
            w,
            h,
            pixels: vec![0; (w * h) as usize],
        }
    }

    fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }
}

/// Which pixels of a shaped window show: one flag per pixel of the canvas.
pub struct Mask {
    bits: Vec<bool>,
    /// Whether every pixel shows: such a shape cuts nothing.
    full: bool,
    /// For every row, the columns from the first pixel that shows to after the last, and
    /// whether every pixel between them shows. A window's shape is most often its box with the
    /// corners cut, so a row is one run, and drawing it asks about no single pixel.
    rows: Vec<(i32, i32, bool)>,
}

impl Mask {
    /// Works out what a row holds, once the bits are final.
    fn seal(&mut self, width: i32) {
        self.full = self.bits.iter().all(|b| *b);
        self.rows = self
            .bits
            .chunks(width.max(1) as usize)
            .map(|row| {
                let first = row.iter().position(|b| *b);
                let last = row.iter().rposition(|b| *b);
                match (first, last) {
                    (Some(first), Some(last)) => {
                        let solid = row[first..=last].iter().all(|b| *b);
                        (first as i32, last as i32 + 1, solid)
                    }
                    _ => (0, 0, true),
                }
            })
            .collect();
    }

    /// The part of columns `left..right` of row `y` that shows, when the row is one run:
    /// `None` asks the caller to go pixel by pixel.
    #[inline]
    fn run(&self, y: i32, left: i32, right: i32) -> Option<(i32, i32)> {
        let (first, last, solid) = self.rows[y as usize];
        solid.then(|| (left.max(first), right.min(last)))
    }
}

/// The shape a window was last drawn with, kept so that a window whose shape did not change
/// does not work it out again.
#[derive(Default)]
pub struct ShapeCache {
    last: Option<(Region, Rc<Mask>)>,
}

/// Where commands land: the clip stack and the window's shape while one is open.
struct Pen {
    clips: Vec<Rect>,
    mask: Option<Rc<Mask>>,
    /// For a stretched picture: which column of the picture each column of the canvas reads.
    columns: Vec<i32>,
}

impl Pen {
    fn clip(&self) -> Rect {
        *self.clips.last().expect("the window's own clip")
    }
}

/// Lays `rgb` (not multiplied) over the pixel at `dst` with alpha `a`, 0..255.
#[inline(always)]
fn over(dst: &mut u32, rgb: u32, a: u32) {
    if a >= 255 {
        *dst = 0xff00_0000 | (rgb & 0x00ff_ffff);
        return;
    }
    let under = *dst;
    let rest = 255 - a;
    let mix = |shift: u32| {
        let s = (rgb >> shift) & 0xff;
        let d = (under >> shift) & 0xff;
        ((s * a + d * rest) / 255) << shift
    };
    let alpha = a + (under >> 24) * rest / 255;
    *dst = (alpha << 24) | mix(16) | mix(8) | mix(0);
}

/// Draws a window's commands into `canvas`, which is cleared first.
pub fn draw_window(
    canvas: &mut Canvas,
    assets: &mut Assets,
    shapes: &mut ShapeCache,
    commands: &[DrawCmd],
) {
    canvas.pixels.fill(0);
    let mut pen = Pen {
        clips: vec![canvas.bounds()],
        mask: None,
        columns: Vec::new(),
    };
    for command in commands {
        draw_command(canvas, assets, shapes, &mut pen, command);
    }
}

fn draw_command(
    canvas: &mut Canvas,
    assets: &mut Assets,
    shapes: &mut ShapeCache,
    pen: &mut Pen,
    command: &DrawCmd,
) {
    match command {
        DrawCmd::Image {
            bitmap,
            dst,
            alpha,
            gamma,
            ..
        } => {
            let tint = gamma.filter(|g| !g.is_identity());
            let Some(pic) = assets.picture_tinted(&bitmap.file, tint) else {
                return;
            };
            // the part of the picture, or the rest of it from its corner
            let src = Rect::new(
                bitmap.x,
                bitmap.y,
                bitmap.w.unwrap_or(pic.w - bitmap.x),
                bitmap.h.unwrap_or(pic.h - bitmap.y),
            );
            stretch(canvas, pen, *dst, src, *alpha, pic.w, pic.h, &pic.argb, pic.opaque);
        }
        DrawCmd::Fill { rect, color, alpha } => fill(canvas, pen, *rect, color.0, *alpha),
        DrawCmd::Pixels {
            rect,
            w,
            h,
            argb,
            alpha,
            ..
        } => {
            if argb.len() >= (*w.max(&0) * *h.max(&0)) as usize {
                stretch(canvas, pen, *rect, Rect::new(0, 0, *w, *h), *alpha, *w, *h, argb, false);
            }
        }
        DrawCmd::Text(text) => draw_text(canvas, assets, pen, text),
        DrawCmd::Clip(rect) => {
            let inner = rect.cut_to(&pen.clip());
            pen.clips.push(inner);
        }
        DrawCmd::Unclip => {
            // the window's own clip stays
            if pen.clips.len() > 1 {
                pen.clips.pop();
            }
        }
        DrawCmd::WindowStart(region) => {
            let mask = match &shapes.last {
                Some((known, mask)) if known == region && mask.bits.len() == canvas.pixels.len() => {
                    mask.clone()
                }
                _ => {
                    let mut mask = rasterize(region, canvas.bounds(), assets);
                    mask.seal(canvas.w);
                    let mask = Rc::new(mask);
                    shapes.last = Some((region.clone(), mask.clone()));
                    mask
                }
            };
            // a shape that is the whole window cuts nothing: no pixel needs asking
            pen.mask = (!mask.full).then_some(mask);
        }
        DrawCmd::WindowEnd => pen.mask = None,
        // the engine spells bitmap text out as clips and images before the host sees it
        DrawCmd::BitmapText(_) => {}
    }
}

/// Fills `rect` with `rgb` at `alpha`.
fn fill(canvas: &mut Canvas, pen: &Pen, rect: Rect, rgb: u32, alpha: i32) {
    let d = rect.cut_to(&pen.clip());
    let a = alpha.clamp(0, 255) as u32;
    if d.width <= 0 || d.height <= 0 || a == 0 {
        return;
    }
    let w = canvas.w as usize;
    for y in d.top..d.bottom() {
        let start = y as usize * w + d.left as usize;
        let row = &mut canvas.pixels[start..start + d.width as usize];
        match &pen.mask {
            None if a == 255 => row.fill(0xff00_0000 | (rgb & 0x00ff_ffff)),
            None => row.iter_mut().for_each(|p| over(p, rgb, a)),
            Some(mask) => match mask.run(y, d.left, d.right()) {
                Some((from, to)) if from < to => {
                    let run = &mut row[(from - d.left) as usize..(to - d.left) as usize];
                    if a == 255 {
                        run.fill(0xff00_0000 | (rgb & 0x00ff_ffff));
                    } else {
                        run.iter_mut().for_each(|p| over(p, rgb, a));
                    }
                }
                Some(_) => {}
                None => {
                    let shown = &mask.bits[start..start + d.width as usize];
                    for (p, show) in row.iter_mut().zip(shown) {
                        if *show {
                            over(p, rgb, a);
                        }
                    }
                }
            },
        }
    }
}

/// Draws the `src` part of a picture of `pic_w` x `pic_h` pixels over `dst` with nearest
/// pixels, at `alpha` on top of each pixel's own. `opaque` says the picture has no clear pixel.
#[allow(clippy::too_many_arguments)]
fn stretch(
    canvas: &mut Canvas,
    pen: &mut Pen,
    dst: Rect,
    src: Rect,
    alpha: i32,
    pic_w: i32,
    pic_h: i32,
    data: &[u32],
    opaque: bool,
) {
    let alpha = alpha.clamp(0, 255) as u32;
    if dst.width <= 0 || dst.height <= 0 || src.width <= 0 || src.height <= 0 || alpha == 0 {
        return;
    }
    let visible = dst.cut_to(&pen.clip());
    if visible.width <= 0 || visible.height <= 0 {
        return;
    }
    let w = canvas.w as usize;
    // most of a skin is opaque pictures at their own size: those rows are copied as they are
    let first = src.left + (visible.left - dst.left);
    let runs = pen.mask.as_ref().is_none_or(|m| {
        (visible.top..visible.bottom()).all(|y| m.rows[y as usize].2)
    });
    if opaque && alpha == 255 && runs && dst.width == src.width && first >= 0 && first + visible.width <= pic_w
    {
        for y in visible.top..visible.bottom() {
            let sy = src.top + (y - dst.top) * src.height / dst.height;
            if sy < 0 || sy >= pic_h {
                continue;
            }
            // the part of the row inside the window's shape
            let (left, right) = match &pen.mask {
                Some(mask) => mask.run(y, visible.left, visible.right()).unwrap_or((0, 0)),
                None => (visible.left, visible.right()),
            };
            if left >= right {
                continue;
            }
            let from = (sy * pic_w + first + (left - visible.left)) as usize;
            let to = y as usize * w + left as usize;
            let n = (right - left) as usize;
            canvas.pixels[to..to + n].copy_from_slice(&data[from..from + n]);
        }
        return;
    }
    // which column of the picture each column drawn reads; -1 outside the picture
    pen.columns.clear();
    pen.columns.extend((visible.left..visible.right()).map(|x| {
        let sx = src.left + (x - dst.left) * src.width / dst.width;
        if sx >= 0 && sx < pic_w {
            sx
        } else {
            -1
        }
    }));
    for y in visible.top..visible.bottom() {
        let sy = src.top + (y - dst.top) * src.height / dst.height;
        if sy < 0 || sy >= pic_h {
            continue;
        }
        let from = &data[(sy * pic_w) as usize..((sy + 1) * pic_w) as usize];
        let start = y as usize * w + visible.left as usize;
        let row = &mut canvas.pixels[start..start + visible.width as usize];
        let shown = pen
            .mask
            .as_ref()
            .map(|m| &m.bits[start..start + visible.width as usize]);
        for (i, (p, &sx)) in row.iter_mut().zip(&pen.columns).enumerate() {
            if sx < 0 || shown.is_some_and(|s| !s[i]) {
                continue;
            }
            let argb = from[sx as usize];
            let a = (argb >> 24) * alpha / 255;
            if a != 0 {
                over(p, argb, a);
            }
        }
    }
}

/// Draws a run of text where the engine placed it: from `x` along the `baseline`, clipped to
/// its box. The shadow goes first, under the letters.
fn draw_text(canvas: &mut Canvas, assets: &mut Assets, pen: &mut Pen, text: &TextCmd) {
    // smoothed text as Winamp draws a modern skin's, the rest as Windows draws it
    let style = if text.smooth {
        Style::Modern
    } else {
        Style::Classic
    };
    let Some(face) = assets.face(text.font.as_ref(), text.bold, style) else {
        return;
    };
    let px = em(style, text.size);
    // Windows places text on whole pixels
    let left = text.x.round() as i32;
    let baseline = text.baseline.round() as i32;
    let clip = text.rect.cut_to(&pen.clip());
    if clip.width <= 0 || clip.height <= 0 {
        return;
    }
    let alpha = text.alpha.clamp(0, 255) as u32;
    // a font without a bold cut is drawn twice, a pixel apart
    let strokes: &[i32] = if face.fake_bold { &[0, 1] } else { &[0] };
    let letters = face
        .font
        .run(&text.text, style, px, i32::from(face.fake_bold));
    let shadow = text
        .shadow
        .map(|s| (left + text.shadow_dx, baseline + text.shadow_dy, s.0));
    let w = canvas.w;
    for (x0, y0, color) in shadow.into_iter().chain([(left, baseline, text.color.0)]) {
        for &dx in strokes {
            for (at, glyph) in &letters {
                let (gx, gy) = (x0 + dx + at + glyph.left, y0 - glyph.top);
                // the letter's box, cut to the clip
                let rows = (clip.top - gy).max(0)..(clip.bottom() - gy).min(glyph.h as i32);
                let cols = (clip.left - gx).max(0)..(clip.right() - gx).min(glyph.w as i32);
                for row in rows {
                    for col in cols.clone() {
                        let cover = u32::from(glyph.coverage[row as usize * glyph.w + col as usize]);
                        if cover == 0 {
                            continue;
                        }
                        let i = ((gy + row) * w + gx + col) as usize;
                        if pen.mask.as_ref().is_some_and(|m| !m.bits[i]) {
                            continue;
                        }
                        let a = cover * alpha / 255;
                        let p = &mut canvas.pixels[i];
                        // a letter Windows smooths blends with its gamma over an opaque pixel
                        if glyph.gamma != 1.0 && (*p >> 24) == 0xff {
                            let mixed = walkit::truetype::blend(
                                color & 0x00ff_ffff,
                                *p & 0x00ff_ffff,
                                a as u8,
                                glyph.gamma,
                            );
                            *p = 0xff00_0000 | mixed;
                        } else if a != 0 {
                            over(p, color, a);
                        }
                    }
                }
            }
        }
    }
}

/// `argb` in the color theme's tint.
pub fn tinted(argb: u32, gamma: Gamma) -> u32 {
    gamma.apply(argb)
}

/// The window's shape as a mask over `area`: a picture's opaque pixels, a box, or a base kept
/// to some regions, with others added and others cut away.
fn rasterize(region: &Region, area: Rect, assets: &mut Assets) -> Mask {
    let mut mask = Mask {
        bits: vec![false; (area.width * area.height) as usize],
        full: false,
        rows: Vec::new(),
    };
    let merge = |into: &mut Mask, other: &Mask, op: fn(bool, bool) -> bool| {
        for (a, b) in into.bits.iter_mut().zip(&other.bits) {
            *a = op(*a, *b);
        }
    };
    match region {
        Region::Box(r) => {
            let d = r.cut_to(&area);
            for y in d.top..d.bottom() {
                let start = (y * area.width + d.left) as usize;
                mask.bits[start..start + d.width.max(0) as usize].fill(true);
            }
        }
        Region::Pic(img) => {
            let Some(pic) = assets.picture(&img.bitmap.file) else {
                return mask;
            };
            let b = &img.bitmap;
            let src = Rect::new(
                b.x,
                b.y,
                b.w.unwrap_or(pic.w - b.x),
                b.h.unwrap_or(pic.h - b.y),
            );
            let d = img.dst;
            if d.width <= 0 || d.height <= 0 || src.width <= 0 || src.height <= 0 {
                return mask;
            }
            let visible = d.cut_to(&area);
            for y in visible.top..visible.bottom() {
                let sy = src.top + (y - d.top) * src.height / d.height;
                for x in visible.left..visible.right() {
                    let sx = src.left + (x - d.left) * src.width / d.width;
                    if pic.at(sx, sy).is_some_and(DrawCmd::is_cut_pixel) {
                        mask.bits[(y * area.width + x) as usize] = true;
                    }
                }
            }
        }
        Region::Op {
            base,
            add,
            keep,
            cut,
        } => {
            if let Some(base) = base {
                mask = rasterize(base, area, assets);
            }
            if !keep.is_empty() {
                let mut kept = Mask {
                    bits: vec![false; mask.bits.len()],
                    full: false,
                    rows: Vec::new(),
                };
                for k in keep {
                    merge(&mut kept, &rasterize(k, area, assets), |a, b| a || b);
                }
                merge(&mut mask, &kept, |a, b| a && b);
            }
            for a in add {
                merge(&mut mask, &rasterize(a, area, assets), |a, b| a || b);
            }
            for c in cut {
                merge(&mut mask, &rasterize(c, area, assets), |a, b| a && !b);
            }
        }
    }
    mask
}
