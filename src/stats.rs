use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, Align2, Color32, FontId, Mesh, Rect, RepaintCause, Sense, Shape, Stroke, TextFormat,
    pos2, vec2,
};

const WINDOW: usize = 240;
const BUDGET_MS: f32 = 1000.0 / 60.0;
const REFRESH: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub last: f32,
    pub mean: f32,
    pub p95: f32,
    pub max: f32,
}

pub fn summarize(values: impl IntoIterator<Item = f32>) -> Option<Summary> {
    let values: Vec<f32> = values.into_iter().collect();
    let last = *values.last()?;
    let mut sorted = values.clone();
    sorted.sort_by(f32::total_cmp);
    let p95 = ((sorted.len() - 1) as f32 * 0.95).round() as usize;
    Some(Summary {
        last,
        mean: values.iter().sum::<f32>() / values.len() as f32,
        p95: sorted[p95],
        max: sorted[sorted.len() - 1],
    })
}

#[derive(Default)]
struct Series(VecDeque<f32>);

impl Series {
    fn push(&mut self, value: f32) {
        if self.0.len() == WINDOW {
            self.0.pop_front();
        }
        self.0.push_back(value);
    }

    fn summary(&self) -> Option<Summary> {
        summarize(self.0.iter().copied())
    }
}

pub enum Value {
    Count(u64),
    Bytes(f64),
    Millis(f32),
    Text(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::Count(n) => write!(f, "{n}"),
            Value::Bytes(b) if *b < 1024.0 => write!(f, "{b:.0} B"),
            Value::Bytes(b) if *b < 1024.0 * 1024.0 => write!(f, "{:.1} KB", b / 1024.0),
            Value::Bytes(b) => write!(f, "{:.1} MB", b / (1024.0 * 1024.0)),
            Value::Millis(ms) => write!(f, "{ms:.1} ms"),
            Value::Text(text) => write!(f, "{text}"),
        }
    }
}

struct Reading {
    section: &'static str,
    name: &'static str,
    value: Value,
}

#[derive(Default)]
pub struct Stats {
    on: bool,
    frames: u64,
    cpu: Series,
    ui: Series,
    paint: Series,
    ui_started: Option<Instant>,
    last_ui: Option<f32>,
    ticks: VecDeque<Instant>,
    phases: Vec<(&'static str, Series)>,
    this_frame: Vec<(&'static str, f32)>,
    readings: Vec<Reading>,
    readout: Option<Readout>,
}

struct Readout {
    made: Instant,
    above_graph: LayoutJob,
    below_graph: LayoutJob,
}

impl Stats {
    pub fn new(on: bool) -> Stats {
        Stats {
            on,
            ..Stats::default()
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn begin_frame(&mut self, cpu_seconds: Option<f32>) {
        if self.on {
            self.begin_frame_at(Instant::now(), cpu_seconds);
        }
    }

    fn begin_frame_at(&mut self, now: Instant, cpu_seconds: Option<f32>) {
        self.frames += 1;
        self.ui_started = Some(now);
        self.ticks.push_back(now);
        while self
            .ticks
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(1))
        {
            self.ticks.pop_front();
        }
        if let Some(cpu) = cpu_seconds.map(|s| s * 1000.0) {
            self.cpu.push(cpu);
            if let Some(ui) = self.last_ui {
                self.paint.push((cpu - ui).max(0.0));
            }
        }
        self.this_frame.clear();
        self.readings.clear();
    }

    pub fn end_frame(&mut self) {
        let Some(started) = self.ui_started.take() else {
            return;
        };
        let ui = ms(started.elapsed());
        self.ui.push(ui);
        self.last_ui = Some(ui);
        for &(phase, took) in &self.this_frame {
            match self.phases.iter_mut().find(|(name, _)| *name == phase) {
                Some((_, series)) => series.push(took),
                None => {
                    let mut series = Series::default();
                    series.push(took);
                    self.phases.push((phase, series));
                }
            }
        }
    }

    pub fn time<R>(&mut self, phase: &'static str, f: impl FnOnce() -> R) -> R {
        if !self.on {
            return f();
        }
        let started = Instant::now();
        let result = f();
        self.record(phase, started.elapsed());
        result
    }

    pub fn record(&mut self, phase: &'static str, took: Duration) {
        if !self.on {
            return;
        }
        match self.this_frame.iter_mut().find(|(name, _)| *name == phase) {
            Some((_, total)) => *total += ms(took),
            None => self.this_frame.push((phase, ms(took))),
        }
    }

    pub fn count(&mut self, section: &'static str, name: &'static str, value: usize) {
        self.set(section, name, Value::Count(value as u64));
    }

    pub fn set(&mut self, section: &'static str, name: &'static str, value: Value) {
        if self.on {
            self.readings.push(Reading {
                section,
                name,
                value,
            });
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, canvas: Rect) {
        // egui only draws when something changes, which would freeze the readout while idle.
        ctx.request_repaint_after(REFRESH);
        if self
            .readout
            .as_ref()
            .is_none_or(|r| r.made.elapsed() >= REFRESH)
        {
            self.readout = Some(self.read_out(ctx, canvas));
        }
        let Some(readout) = &self.readout else {
            return;
        };
        egui::Area::new(egui::Id::unique("stats overlay"))
            .order(egui::Order::Foreground)
            .interactable(false)
            .pivot(Align2::RIGHT_TOP)
            .fixed_pos(canvas.right_top() + vec2(-10.0, 10.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(readout.above_graph.clone());
                    sparkline(ui, &self.cpu);
                    ui.label(readout.below_graph.clone());
                });
            });
    }

    fn read_out(&self, ctx: &egui::Context, canvas: Rect) -> Readout {
        let mut above = vec![Line::plain(self.headline(ctx, canvas))];
        let frame = [("cpu", &self.cpu), ("ui", &self.ui), ("paint", &self.paint)];
        above.extend(timing_lines("frame ms", frame));
        let phases = self.phases.iter().map(|(name, series)| (*name, series));
        let mut below = timing_lines("phase ms", phases);
        below.extend(self.reading_lines());
        below.extend(cause_lines(&ctx.repaint_causes()));
        let visuals = ctx.global_style().visuals.clone();
        Readout {
            made: Instant::now(),
            above_graph: layout(&above, &visuals),
            below_graph: layout(&below, &visuals),
        }
    }

    fn headline(&self, ctx: &egui::Context, canvas: Rect) -> String {
        let build = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        format!(
            "{build} · {}x · {:.0}×{:.0} · {} fps · frame {}",
            ctx.pixels_per_point(),
            canvas.width(),
            canvas.height(),
            self.ticks.len(),
            self.frames
        )
    }

    fn reading_lines(&self) -> Vec<Line> {
        let mut sections: Vec<&str> = Vec::new();
        for reading in &self.readings {
            if !sections.contains(&reading.section) {
                sections.push(reading.section);
            }
        }
        let mut lines = Vec::new();
        for section in sections {
            lines.push(Line::weak(section.to_string()));
            let readings: Vec<&Reading> = self
                .readings
                .iter()
                .filter(|r| r.section == section)
                .collect();
            for pair in readings.chunks(2) {
                let cells: Vec<String> = pair
                    .iter()
                    .map(|r| format!("{:<10}{:>9}", r.name, r.value.to_string()))
                    .collect();
                lines.push(Line::plain(cells.join("  ")));
            }
        }
        lines
    }
}

struct Line {
    text: String,
    weak: bool,
}

impl Line {
    fn plain(text: String) -> Line {
        Line { text, weak: false }
    }

    fn weak(text: String) -> Line {
        Line { text, weak: true }
    }
}

fn timing_lines<'a>(
    title: &str,
    rows: impl IntoIterator<Item = (&'a str, &'a Series)>,
) -> Vec<Line> {
    let heading = format!(
        "{title:<10}{:>8}{:>8}{:>8}{:>8}",
        "last", "avg", "p95", "max"
    );
    let mut lines = vec![Line::weak(heading)];
    for (name, series) in rows {
        if let Some(s) = series.summary() {
            let row = format!(
                "{name:<10}{:>8.2}{:>8.2}{:>8.2}{:>8.2}",
                s.last, s.mean, s.p95, s.max
            );
            lines.push(Line::plain(row));
        }
    }
    lines
}

fn cause_lines(causes: &[RepaintCause]) -> Vec<Line> {
    let mut causes: Vec<String> = causes.iter().map(short_cause).collect();
    causes.dedup();
    if causes.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::weak("repainted because of".into())];
    lines.extend(causes.into_iter().map(Line::plain));
    lines
}

fn layout(lines: &[Line], visuals: &egui::Visuals) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, line) in lines.iter().enumerate() {
        let color = if line.weak {
            visuals.weak_text_color()
        } else {
            visuals.text_color()
        };
        let format = TextFormat::simple(FontId::monospace(11.0), color);
        let end = if i + 1 < lines.len() { "\n" } else { "" };
        job.append(&format!("{}{end}", line.text), 0.0, format);
    }
    job
}

fn ms(took: Duration) -> f32 {
    took.as_secs_f32() * 1000.0
}

fn short_cause(cause: &RepaintCause) -> String {
    let mut parts: Vec<&str> = cause.file.rsplit(['/', '\\']).take(3).collect();
    parts.reverse();
    let at = format!("{}:{}", parts.join("/"), cause.line);
    if cause.reason.is_empty() {
        at
    } else {
        format!("{at} {}", cause.reason)
    }
}

fn sparkline(ui: &mut egui::Ui, cpu: &Series) {
    let (rect, _) = ui.allocate_exact_size(vec2(WINDOW as f32, 32.0), Sense::hover());
    let top = cpu.0.iter().copied().fold(2.0 * BUDGET_MS, f32::max);
    let y = |ms: f32| rect.bottom() - rect.height() * ms / top;
    let mut bars = Mesh::default();
    for (i, &ms) in cpu.0.iter().enumerate() {
        let x = rect.left() + i as f32;
        let bar = Rect::from_min_max(pos2(x, y(ms)), pos2(x + 1.0, rect.bottom()));
        bars.add_colored_rect(bar, bar_color(ms));
    }
    ui.painter().add(Shape::mesh(bars));
    let budget = Stroke::new(1.0, Color32::from_gray(120));
    ui.painter().hline(rect.x_range(), y(BUDGET_MS), budget);
}

fn bar_color(ms: f32) -> Color32 {
    if ms <= BUDGET_MS {
        Color32::from_rgb(90, 190, 110)
    } else if ms <= 2.0 * BUDGET_MS {
        Color32::from_rgb(230, 170, 50)
    } else {
        Color32::from_rgb(230, 80, 70)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_of_known_values() {
        let s = summarize((1..=100).map(|v| v as f32)).unwrap();
        assert_eq!(
            s,
            Summary {
                last: 100.0,
                mean: 50.5,
                p95: 95.0,
                max: 100.0
            }
        );
        assert_eq!(summarize([]), None);
    }

    #[test]
    fn series_keep_the_last_window() {
        let mut series = Series::default();
        for v in 0..WINDOW + 10 {
            series.push(v as f32);
        }
        assert_eq!(series.0.len(), WINDOW);
        assert_eq!(series.0.front(), Some(&10.0));
    }

    #[test]
    fn values_read_like_units() {
        let shown = [
            Value::Count(42),
            Value::Bytes(512.0),
            Value::Bytes(12.5 * 1024.0),
            Value::Bytes(3.3 * 1024.0 * 1024.0),
            Value::Millis(7.3),
            Value::Text("direct".into()),
        ]
        .map(|v| v.to_string());
        assert_eq!(
            shown,
            ["42", "512 B", "12.5 KB", "3.3 MB", "7.3 ms", "direct"]
        );
    }

    #[test]
    fn fps_counts_frames_of_the_last_second() {
        let mut stats = Stats::new(true);
        let start = Instant::now();
        for i in 0..90 {
            stats.begin_frame_at(start + Duration::from_millis(i * 20), None);
            stats.end_frame();
        }
        assert_eq!(stats.ticks.len(), 50);
    }

    #[test]
    fn paint_is_the_cpu_time_outside_ui() {
        let mut stats = Stats::new(true);
        let start = Instant::now();
        stats.begin_frame_at(start, None);
        stats.end_frame();
        let ui = stats.last_ui.unwrap();
        stats.begin_frame_at(start + Duration::from_millis(16), Some(0.010));
        let paint = stats.paint.summary().unwrap().last;
        assert!((paint - (10.0 - ui)).abs() < 1e-3);
    }

    #[test]
    fn phases_add_up_within_a_frame() {
        let mut stats = Stats::new(true);
        stats.begin_frame_at(Instant::now(), None);
        stats.record("routing", Duration::from_millis(2));
        stats.record("routing", Duration::from_millis(3));
        stats.end_frame();
        assert_eq!(stats.this_frame, [("routing", 5.0)]);
        assert_eq!(stats.phases[0].1.summary().unwrap().last, 5.0);
    }

    #[test]
    fn the_overlay_keeps_itself_current_while_idle() {
        let ctx = egui::Context::default();
        let mut stats = Stats::new(true);
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let mut delay = Duration::MAX;
        for second in 0..3 {
            let input = egui::RawInput {
                time: Some(second as f64),
                ..Default::default()
            };
            let mut out = ctx.run_pass(input, |ctx| stats.show(ctx, canvas));
            out.textures_delta.clear();
            delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        }
        assert!(delay <= REFRESH, "next repaint in {delay:?}");
    }

    #[test]
    fn nothing_is_collected_when_off() {
        let mut stats = Stats::new(false);
        stats.begin_frame(Some(0.01));
        let routed = stats.time("routing", || 7);
        stats.count("scene", "tables", 3);
        stats.end_frame();
        assert_eq!(routed, 7);
        assert_eq!((stats.frames, stats.ticks.len()), (0, 0));
        assert!(stats.cpu.0.is_empty() && stats.ui.0.is_empty() && stats.phases.is_empty());
        assert!(stats.this_frame.is_empty() && stats.readings.is_empty());
    }
}
