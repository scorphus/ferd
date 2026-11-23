use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::emath::TSTransform;
use eframe::egui::epaint::{CornerRadiusF32, RectShape};
use eframe::egui::layers::ShapeIdx;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2,
};

const TEXT_SETTLES: Duration = Duration::from_millis(150);
const TEXT_AHEAD: f32 = 1.5;
const CANVAS_FONT: &str = "canvas";
const CANVAS_MONO: &str = "canvas mono";

pub struct Pen<'a> {
    painter: &'a egui::Painter,
    to_screen: TSTransform,
    text_zoom: f32,
    texts: Vec<(ShapeIdx, TSTransform)>,
    lines_of: Vec<(FontId, Line)>,
    proportional: egui::FontFamily,
    monospace: egui::FontFamily,
}

#[derive(Clone, Copy)]
struct Line {
    height: f32,
    baseline: f32,
}

impl<'a> Pen<'a> {
    pub fn new(painter: &'a egui::Painter, to_screen: TSTransform, text_zoom: f32) -> Pen<'a> {
        Pen {
            painter,
            to_screen,
            text_zoom,
            texts: Vec::new(),
            lines_of: Vec::new(),
            proportional: egui::FontFamily::Name(CANVAS_FONT.into()),
            monospace: egui::FontFamily::Name(CANVAS_MONO.into()),
        }
    }

    fn unhinted(&self, font: FontId) -> FontId {
        let family = match font.family {
            egui::FontFamily::Monospace => self.monospace.clone(),
            _ => self.proportional.clone(),
        };
        FontId::new(font.size, family)
    }

    pub fn zoom(&self) -> f32 {
        self.to_screen.scaling
    }

    fn screen(&self, world: &[Pos2]) -> Vec<Pos2> {
        world.iter().map(|&p| self.to_screen * p).collect()
    }

    pub fn rect(
        &self,
        world: Rect,
        corners: impl Into<CornerRadiusF32>,
        fill: Color32,
        line: Stroke,
    ) {
        let screen = self.to_screen * world;
        let corners = corners.into() * self.zoom();
        let shape = RectShape::new(screen, corners, fill, line, StrokeKind::Inside);
        self.painter.add(shape.with_round_to_pixels(false));
    }

    pub fn line(&self, world: &[Pos2], line: Stroke) {
        self.painter.add(Shape::line(self.screen(world), line));
    }

    pub fn dashed_line(&self, world: &[Pos2], line: Stroke, dash: f32, gap: f32) {
        let (dash, gap) = (dash * self.zoom(), gap * self.zoom());
        let dashes = Shape::dashed_line(&self.screen(world), line, dash, gap);
        self.painter.extend(dashes);
    }

    pub fn segment(&self, world: [Pos2; 2], line: Stroke) {
        let [a, b] = world;
        self.painter
            .line_segment([self.to_screen * a, self.to_screen * b], line);
    }

    pub fn circle(&self, center: Pos2, radius: f32, fill: Color32, line: Stroke) {
        let (center, radius) = (self.to_screen * center, radius * self.zoom());
        self.painter.circle(center, radius, fill, line);
    }

    pub fn text(&mut self, at: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
        let font = self.unhinted(font);
        let line = self.line_of(&font);
        let laid_out = FontId::new(font.size * self.text_zoom, font.family);
        let galley = self
            .painter
            .layout_no_wrap(text.to_string(), laid_out, color);
        let scale = self.zoom() / self.text_zoom;
        let left = align
            .anchor_size(self.to_screen * at, galley.size() * scale)
            .min
            .x;
        let top = align.anchor_size(at, vec2(0.0, line.height)).min.y;
        let baseline = (self.to_screen * pos2(at.x, top + line.baseline)).y;
        let place = vec2(left, baseline - first_baseline(&galley) * scale);
        let shape = self.painter.add(Shape::galley(Pos2::ZERO, galley, color));
        self.texts.push((shape, TSTransform::new(place, scale)));
    }

    fn line_of(&mut self, font: &FontId) -> Line {
        if let Some((_, line)) = self.lines_of.iter().find(|(f, _)| f == font) {
            return *line;
        }
        let sample = self
            .painter
            .layout_no_wrap("x".into(), font.clone(), Color32::WHITE);
        let line = Line {
            height: sample.size().y,
            baseline: first_baseline(&sample),
        };
        self.lines_of.push((font.clone(), line));
        line
    }

    pub fn finish(self) {
        let layer = self.painter.layer_id();
        self.painter.ctx().graphics_mut(|g| {
            let shapes = g.entry(layer);
            for (shape, transform) in self.texts {
                shapes.mutate_shape(shape, |s| s.transform_after_tessellation = transform);
            }
        });
    }
}

pub fn canvas_fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let families = [
        (egui::FontFamily::Proportional, CANVAS_FONT),
        (egui::FontFamily::Monospace, CANVAS_MONO),
    ];
    for (family, name) in families {
        let mut unhinted = Vec::new();
        for font in fonts.families[&family].clone() {
            let mut data = (*fonts.font_data[&font]).clone();
            data.tweak.hinting = Some(false);
            let copy = format!("{font} unhinted");
            fonts.font_data.insert(copy.clone(), Arc::new(data));
            unhinted.push(copy);
        }
        fonts
            .families
            .insert(egui::FontFamily::Name(name.into()), unhinted);
    }
    fonts
}

// Text drawn at a new size lands on the pixel grid differently, so drawing it anew at every
// zoom makes it wiggle. While zooming, it is drawn at most once per 1.5x and only ever
// scaled down, which keeps it sharp; once the zoom settles, it is drawn at the exact size.
#[derive(Default)]
pub struct TextZoom {
    current: f32,
    last_zoom: f32,
    moved_at: Option<Instant>,
}

impl TextZoom {
    pub fn follow(&mut self, zoom: f32, now: Instant, exporting: bool) -> f32 {
        let first = self.current == 0.0;
        if zoom != self.last_zoom && !first {
            self.moved_at = Some(now);
        }
        self.last_zoom = zoom;
        let settled = self.moved_at.is_none_or(|at| now - at >= TEXT_SETTLES);
        let scale = zoom / self.current;
        if first || exporting || settled {
            self.current = zoom;
        } else if scale > 1.0 {
            self.current = zoom * TEXT_AHEAD;
        } else if scale < 1.0 / TEXT_AHEAD {
            self.current = zoom;
        }
        self.current
    }

    pub fn settles_in(&self, now: Instant) -> Option<Duration> {
        if self.current == self.last_zoom {
            return None;
        }
        Some(TEXT_SETTLES.saturating_sub(now - self.moved_at?))
    }
}

fn first_baseline(galley: &egui::Galley) -> f32 {
    let Some(row) = galley.rows.first() else {
        return 0.0;
    };
    row.pos.y + row.glyphs.first().map_or(0.0, |glyph| glyph.pos.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FONT, View};
    use eframe::egui::ColorImage;

    #[test]
    fn text_is_only_scaled_down_while_zooming_and_exact_after() {
        let start = Instant::now();
        let mut text = TextZoom::default();
        assert_eq!(text.follow(1.0, start, false), 1.0);
        let (mut zoom, mut at, mut redraws) = (1.0f32, start, 0);
        for step in 0..120 {
            zoom *= if step < 60 { 1.02 } else { 1.0 / 1.02 };
            at += Duration::from_millis(16);
            let before = text.current;
            let laid_out = text.follow(zoom, at, false);
            assert!(
                zoom <= laid_out && laid_out <= zoom * TEXT_AHEAD,
                "{zoom} drawn at {laid_out}"
            );
            redraws += usize::from(laid_out != before);
        }
        assert!(redraws <= 6, "{redraws} redraws");
        assert_eq!(text.settles_in(at), Some(TEXT_SETTLES));
        let settled = at + TEXT_SETTLES;
        assert_eq!(text.follow(zoom, settled, false), zoom);
        assert_eq!(text.settles_in(settled), None);
        let exported = zoom * 1.3;
        assert_eq!(text.follow(exported, settled, true), exported);
    }

    #[test]
    fn glyph_heights_follow_the_zoom() {
        let views: Vec<View> = small_zoom_steps()
            .into_iter()
            .filter(|view| view.zoom >= 0.75)
            .collect();
        for pixels_per_point in [1.0, 2.0] {
            let (ctx, mut atlas) = context_with_fonts(pixels_per_point);
            let inks: Vec<Vec<Ink>> = views
                .iter()
                .map(|&view| glyph_inks(&ctx, &mut atlas, view, view.zoom))
                .collect();
            for (pair, views) in inks.windows(2).zip(views.windows(2)) {
                for (a, b) in pair[0].iter().zip(&pair[1]) {
                    let change = (b.height / a.height - 1.0).abs();
                    let flat = a.height < 1.0;
                    let at = format!("at zoom {} on {pixels_per_point}x", views[1].zoom);
                    assert!(
                        flat || change < 0.06,
                        "a glyph's height changed {change} {at}"
                    );
                }
            }
        }
    }

    #[test]
    fn glyphs_hold_still_between_redraws_while_zooming() {
        let views = small_zoom_steps();
        let range = views[views.len() - 1].zoom / views[0].zoom;
        let redraws = (range.ln() / TEXT_AHEAD.ln()).ceil() as usize + 1;
        for pixels_per_point in [1.0, 2.0] {
            let (ctx, mut atlas) = context_with_fonts(pixels_per_point);
            let mut text = TextZoom::default();
            let start = Instant::now();
            let mut inks: Vec<Vec<Ink>> = Vec::new();
            for (step, &view) in views.iter().enumerate() {
                let now = start + Duration::from_millis(16 * step as u64);
                let text_zoom = text.follow(view.zoom, now, false);
                inks.push(glyph_inks(&ctx, &mut atlas, view, text_zoom));
            }
            let mut changes = 0;
            for (pair, views) in inks.windows(2).zip(views.windows(2)) {
                let (moved, grew) = largest_change(&pair[0], &pair[1], views[1].zoom);
                let at = format!("at zoom {} on {pixels_per_point}x", views[1].zoom);
                assert!(moved < 0.75, "a glyph moved {moved} px {at}");
                assert!(grew < 0.75, "a glyph grew by {grew} px {at}");
                changes += usize::from(moved > 0.01 || grew > 0.01);
            }
            assert!(
                changes <= redraws,
                "glyphs changed {changes} times, {redraws} allowed"
            );
        }
    }

    #[derive(Clone, Copy)]
    struct Ink {
        centre: Pos2,
        height: f32,
    }

    #[derive(Default)]
    struct Atlas(Option<ColorImage>);

    impl Atlas {
        fn apply(&mut self, out: &mut egui::FullOutput) {
            for (texture, deltas) in &out.textures_delta.set {
                if *texture == egui::TextureId::default() {
                    deltas.iter().for_each(|delta| self.paste(delta));
                }
            }
            out.textures_delta.clear();
        }

        fn paste(&mut self, delta: &egui::epaint::ImageDelta) {
            let egui::ImageData::Color(patch) = &delta.image;
            let Some([left, top]) = delta.pos else {
                self.0 = Some((**patch).clone());
                return;
            };
            let image = self.0.as_mut().expect("the whole atlas comes first");
            for y in 0..patch.size[1] {
                for x in 0..patch.size[0] {
                    let to = (top + y) * image.size[0] + left + x;
                    image.pixels[to] = patch.pixels[y * patch.size[0] + x];
                }
            }
        }

        fn ink(&self, quad: &[egui::epaint::Vertex], to_world: TSTransform) -> Ink {
            let image = self.0.as_ref().expect("the fonts are in the atlas");
            let size = vec2(image.size[0] as f32, image.size[1] as f32);
            let screen: Vec<Pos2> = quad.iter().map(|v| v.pos).collect();
            let texels: Vec<Pos2> = quad
                .iter()
                .map(|v| (v.uv.to_vec2() * size).to_pos2())
                .collect();
            let (screen, texels) = (Rect::from_points(&screen), Rect::from_points(&texels));
            let (mut mass, mut x, mut y, mut yy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for row in texels.min.y.floor() as usize..texels.max.y.ceil() as usize {
                for column in texels.min.x.floor() as usize..texels.max.x.ceil() as usize {
                    let alpha = image.pixels[row * image.size[0] + column].a() as f64 / 255.0;
                    let texel = pos2(column as f32 + 0.5, row as f32 + 0.5);
                    let along = (texel - texels.min) / texels.size();
                    let world = to_world * (screen.min + along * screen.size());
                    mass += alpha;
                    x += alpha * world.x as f64;
                    y += alpha * world.y as f64;
                    yy += alpha * world.y as f64 * world.y as f64;
                }
            }
            let (x, y) = (x / mass, y / mass);
            Ink {
                centre: pos2(x as f32, y as f32),
                height: (yy / mass - y * y).max(0.0).sqrt() as f32,
            }
        }
    }

    fn glyph_inks(ctx: &egui::Context, atlas: &mut Atlas, view: View, text_zoom: f32) -> Vec<Ink> {
        let canvas = pos2(220.0, 30.0);
        let mut out = ctx.run_pass(Default::default(), |ctx| {
            let mut painter = ctx.layer_painter(egui::LayerId::background());
            painter.set_clip_rect(Rect::EVERYTHING);
            let mut pen = Pen::new(&painter, view.to_screen(canvas), text_zoom);
            let (at, font) = (pos2(37.0, 61.0), FontId::proportional(FONT));
            pen.text(at, Align2::LEFT_CENTER, "Customer_id", font, Color32::WHITE);
            pen.finish();
        });
        atlas.apply(&mut out);
        let primitives = ctx.tessellate(out.shapes, ctx.pixels_per_point());
        let egui::epaint::Primitive::Mesh(mesh) = &primitives[0].primitive else {
            panic!("text is drawn as a mesh");
        };
        let to_world = view.to_world(canvas);
        mesh.vertices
            .chunks(4)
            .map(|quad| atlas.ink(quad, to_world))
            .collect()
    }

    fn largest_change(before: &[Ink], after: &[Ink], zoom: f32) -> (f32, f32) {
        let mut moved = 0.0f32;
        let mut grew = 0.0f32;
        for (a, b) in before.iter().zip(after) {
            moved = moved.max((b.centre - a.centre).abs().max_elem() * zoom);
            grew = grew.max((b.height - a.height).abs() * zoom);
        }
        (moved, grew)
    }

    fn context_with_fonts(pixels_per_point: f32) -> (egui::Context, Atlas) {
        let ctx = egui::Context::default();
        ctx.set_fonts(canvas_fonts());
        ctx.set_pixels_per_point(pixels_per_point);
        let mut atlas = Atlas::default();
        for _ in 0..2 {
            atlas.apply(&mut ctx.run_pass(Default::default(), |_| {}));
        }
        (ctx, atlas)
    }

    fn small_zoom_steps() -> Vec<View> {
        let mut views = Vec::new();
        let mut zoom = 0.5;
        while zoom < 3.0 {
            let offset = vec2(13.37, -7.21) + vec2(3.1, 1.7) * zoom;
            views.push(View { offset, zoom });
            zoom *= 1.005;
        }
        views
    }
}
