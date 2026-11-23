use std::f32::consts::TAU;
use std::fmt::Write;
use std::time::Instant;

use eframe::egui::{Vec2, vec2};

use crate::stats::summarize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    Still,
    Zoom(f32),
    Pan(Vec2),
    Drag(Vec2),
}

#[derive(Clone, Copy)]
enum Step {
    Warmup,
    Still,
    Zoom,
    Pan,
    Drag,
}

// Small enough that zooming in for half the step never reaches the zoom limit from a fitted
// view, so every zoom frame really zooms.
const ZOOM_STEP: f32 = 1.008;

const STEPS: [(Step, usize); 5] = [
    (Step::Warmup, 30),
    (Step::Still, 120),
    (Step::Zoom, 240),
    (Step::Pan, 180),
    (Step::Drag, 180),
];

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::Warmup => "warmup",
            Step::Still => "still",
            Step::Zoom => "zoom",
            Step::Pan => "pan",
            Step::Drag => "drag",
        }
    }

    fn motion(self, frame: usize, frames: usize) -> Motion {
        let back = frame >= frames / 2;
        match self {
            Step::Warmup | Step::Still => Motion::Still,
            Step::Zoom if back => Motion::Zoom(1.0 / ZOOM_STEP),
            Step::Zoom => Motion::Zoom(ZOOM_STEP),
            Step::Pan if back => Motion::Pan(vec2(-6.0, -3.0)),
            Step::Pan => Motion::Pan(vec2(6.0, 3.0)),
            Step::Drag => {
                let turn = TAU / frames as f32;
                let angle = turn * frame as f32;
                Motion::Drag(vec2(-angle.sin(), angle.cos()) * 120.0 * turn)
            }
        }
    }
}

struct Sample {
    step: usize,
    at: Instant,
    cpu: Option<f32>,
    ui: f32,
    phases: Vec<(&'static str, f32)>,
}

#[derive(Default)]
pub struct Bench {
    frame: usize,
    samples: Vec<Sample>,
}

impl Bench {
    pub fn next_motion(&mut self) -> Option<Motion> {
        let (step, frame) = locate(self.frame)?;
        self.frame += 1;
        let (kind, frames) = STEPS[step];
        Some(kind.motion(frame, frames))
    }

    pub fn previous_frame_took(&mut self, cpu_seconds: Option<f32>) {
        if let Some(sample) = self.samples.last_mut() {
            sample.cpu = cpu_seconds.map(|s| s * 1000.0);
        }
    }

    pub fn record(&mut self, ui: f32, phases: &[(&'static str, f32)]) {
        let Some((step, _)) = locate(self.frame.saturating_sub(1)) else {
            return;
        };
        self.samples.push(Sample {
            step,
            at: Instant::now(),
            cpu: None,
            ui,
            phases: phases.to_vec(),
        });
    }

    pub fn report(&self, about: &str) -> String {
        let phases = self.phase_names();
        let mut out = String::new();
        let _ = writeln!(out, "{about}\n");
        let _ = write!(
            out,
            "{:<8}{:>7}{:>7}{:>9}{:>9}{:>9}{:>9}",
            "step", "frames", "fps", "cpu avg", "cpu p95", "cpu max", "ui avg"
        );
        for phase in &phases {
            let _ = write!(out, "{phase:>9}");
        }
        let _ = writeln!(out);
        for (i, (step, _)) in STEPS.iter().enumerate() {
            let samples: Vec<&Sample> = self.samples.iter().filter(|s| s.step == i).collect();
            if !samples.is_empty() {
                let _ = writeln!(out, "{}", step_line(step.name(), &samples, &phases));
            }
        }
        let _ = writeln!(
            out,
            "\nAll times in ms. fps is capped by the display; cpu is what a frame cost \
             without waiting for it. Phases are averaged over the frames they ran in."
        );
        out
    }

    fn phase_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();
        for sample in &self.samples {
            for (name, _) in &sample.phases {
                if !names.contains(name) {
                    names.push(name);
                }
            }
        }
        names
    }
}

fn locate(frame: usize) -> Option<(usize, usize)> {
    let mut start = 0;
    for (i, &(_, frames)) in STEPS.iter().enumerate() {
        if frame < start + frames {
            return Some((i, frame - start));
        }
        start += frames;
    }
    None
}

fn step_line(name: &str, samples: &[&Sample], phases: &[&'static str]) -> String {
    let cpu = summarize(samples.iter().filter_map(|s| s.cpu));
    let ui = summarize(samples.iter().map(|s| s.ui));
    let mut line = format!("{name:<8}{:>7}{:>7}", samples.len(), fps(samples));
    line += &cell(cpu.map(|s| s.mean));
    line += &cell(cpu.map(|s| s.p95));
    line += &cell(cpu.map(|s| s.max));
    line += &cell(ui.map(|s| s.mean));
    for phase in phases {
        let took = samples
            .iter()
            .flat_map(|s| s.phases.iter())
            .filter(|(name, _)| name == phase)
            .map(|&(_, ms)| ms);
        line += &cell(summarize(took).map(|s| s.mean));
    }
    line
}

fn fps(samples: &[&Sample]) -> String {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        return "-".into();
    };
    let seconds = last.at.duration_since(first.at).as_secs_f32();
    if seconds > 0.0 {
        format!("{:.0}", (samples.len() - 1) as f32 / seconds)
    } else {
        "-".into()
    }
}

fn cell(value: Option<f32>) -> String {
    match value {
        Some(v) => format!("{v:>9.2}"),
        None => format!("{:>9}", "-"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_every_step_and_then_stops() {
        let mut bench = Bench::default();
        let mut motions = Vec::new();
        while let Some(motion) = bench.next_motion() {
            motions.push(motion);
            bench.record(1.0, &[("routing", 0.5)]);
        }
        let frames: usize = STEPS.iter().map(|(_, n)| n).sum();
        assert_eq!(motions.len(), frames);
        assert_eq!(bench.samples.len(), frames);
        assert_eq!(bench.next_motion(), None);
    }

    #[test]
    fn zoom_pan_and_drag_come_back_to_the_start() {
        let mut bench = Bench::default();
        let (mut zoom, mut pan, mut drag) = (1.0f32, Vec2::ZERO, Vec2::ZERO);
        while let Some(motion) = bench.next_motion() {
            match motion {
                Motion::Still => {}
                Motion::Zoom(f) => zoom *= f,
                Motion::Pan(by) => pan += by,
                Motion::Drag(by) => drag += by,
            }
        }
        assert!((zoom - 1.0).abs() < 1e-3, "{zoom}");
        assert_eq!(pan, Vec2::ZERO);
        assert!(drag.length() < 1e-2, "{drag:?}");
    }

    #[test]
    fn report_has_a_line_per_step_and_a_column_per_phase() {
        let mut bench = Bench::default();
        bench.previous_frame_took(Some(0.004));
        while bench.next_motion().is_some() {
            bench.previous_frame_took(Some(0.004));
            bench.record(2.0, &[("routing", 0.5), ("tables", 0.25)]);
        }
        let report = bench.report("about");
        let lines: Vec<&str> = report.lines().collect();
        assert!(lines[2].ends_with(" routing   tables"), "{}", lines[2]);
        for (step, (kind, _)) in STEPS.iter().enumerate() {
            assert!(lines[3 + step].starts_with(kind.name()));
        }
        assert!(lines[4].contains("     4.00"), "{}", lines[4]);
    }
}
