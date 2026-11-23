mod arrange;
mod bench;
mod layout;
#[cfg(target_os = "macos")]
mod macos;
mod mermaid;
mod model;
mod pen;
mod route;
mod share;
mod sql;
mod stats;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::emath::TSTransform;
use eframe::egui::epaint::CornerRadiusF32;
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2, pos2, vec2,
};
use notify::{RecursiveMode, Watcher};

use bench::{Bench, Motion};
use layout::{DiagramLayout, LayoutFile};
use model::{Card, Column, Diagram, Link, Table};
use pen::{Pen, TextZoom, canvas_fonts};
use route::{Route, route_links};
use share::{Event, Msg, Session, Tally};
use stats::{Stats, Value};

const FONT: f32 = 13.0;
const LABEL_FONT: f32 = FONT * 0.85;
const SMALLEST_FONT: f32 = 4.0;
const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 19.0;
const PAD: f32 = 10.0;
const TAG_W: f32 = 24.0;
const GAP: f32 = 70.0;
const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 4.0;
const MAX_FIT_ZOOM: f32 = 1.5;

const USAGE: &str = "usage: ferd [--stats] [--bench] [file | --join ticket]";

#[derive(Debug, Default, PartialEq)]
struct Args {
    path: Option<PathBuf>,
    join: Option<String>,
    stats: bool,
    bench: bool,
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--stats") => parsed.stats = true,
            Some("--bench") => parsed.bench = true,
            Some("--join") => {
                let ticket = args.next().ok_or("--join needs a ticket")?;
                parsed.join = Some(ticket.to_string_lossy().into_owned());
            }
            Some(option) if option.starts_with("--") => {
                return Err(format!("unknown option {option}"));
            }
            _ if parsed.path.is_some() => return Err("open one file at a time".into()),
            _ => parsed.path = Some(PathBuf::from(arg)),
        }
    }
    if parsed.path.is_some() && parsed.join.is_some() {
        return Err("open a file or join, not both".into());
    }
    if parsed.bench && parsed.path.is_none() {
        return Err("--bench needs a file".into());
    }
    Ok(parsed)
}

fn main() -> eframe::Result {
    let args = match parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("ferd: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    #[cfg(target_os = "macos")]
    macos::listen();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 860.0])
            .with_title("Ferd")
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Ferd",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(canvas_fonts());
            let mut app = App {
                stats: Stats::new(args.stats || args.bench),
                show_stats: args.stats,
                bench: args.bench.then(Bench::default),
                ..App::default()
            };
            if let Some(path) = args.path {
                app.open(&cc.egui_ctx, path);
            }
            if let Some(ticket) = args.join {
                app.join(&cc.egui_ctx, &ticket);
            }
            Ok(Box::new(app))
        }),
    )
}

struct Source {
    name: String,
    text: String,
    path: Option<PathBuf>,
    diagrams: Vec<(String, Diagram)>,
    selected: usize,
    layout: LayoutFile,
    _watcher: Option<notify::RecommendedWatcher>,
    changes: Option<Receiver<notify::Result<notify::Event>>>,
}

impl Source {
    fn title(&self) -> &str {
        &self.diagrams[self.selected].0
    }

    fn diagram(&self) -> &Diagram {
        &self.diagrams[self.selected].1
    }

    fn diagram_layout(&mut self) -> &mut DiagramLayout {
        let title = self.diagrams[self.selected].0.clone();
        self.layout.diagrams.entry(title).or_default()
    }

    fn links(&self) -> impl Iterator<Item = (&Link, bool)> {
        let hand = self
            .layout
            .diagrams
            .get(self.title())
            .map(|d| d.links.as_slice())
            .unwrap_or(&[]);
        self.diagram()
            .links
            .iter()
            .map(|l| (l, false))
            .chain(hand.iter().map(|l| (l, true)))
    }

    fn save(&self) -> Result<(), String> {
        match &self.path {
            Some(path) => layout::save(path, &self.layout),
            None => Ok(()),
        }
    }

    fn set_text(&mut self, text: String) -> Result<(), String> {
        let diagrams = parse_diagrams(&self.name, &text)?;
        let title = self.diagrams.get(self.selected).map(|d| d.0.clone());
        self.selected = diagrams
            .iter()
            .position(|(t, _)| Some(t) == title.as_ref())
            .unwrap_or(0);
        self.diagrams = diagrams;
        self.text = text;
        Ok(())
    }
}

fn parse_diagrams(name: &str, text: &str) -> Result<Vec<(String, Diagram)>, String> {
    if name.to_ascii_lowercase().ends_with(".sql") {
        return Ok(vec![("schema".to_string(), sql::parse(text))]);
    }
    let blocks = mermaid::blocks(text);
    if blocks.is_empty() {
        return Err(format!("{name}: no mermaid erDiagram block"));
    }
    Ok(blocks.into_iter().map(|b| (b.title, b.diagram)).collect())
}

#[derive(Clone, Copy)]
struct View {
    offset: Vec2,
    zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        View {
            offset: vec2(40.0, 40.0),
            zoom: 1.0,
        }
    }
}

impl View {
    fn to_screen(self, canvas: Pos2) -> TSTransform {
        TSTransform::new(canvas.to_vec2() + self.offset, self.zoom)
    }

    fn to_world(self, canvas: Pos2) -> TSTransform {
        self.to_screen(canvas).inverse()
    }

    fn zoomed(self, factor: f32, canvas: Pos2, pointer: Pos2) -> View {
        let anchor = self.to_world(canvas) * pointer;
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let mut view = View { zoom, ..self };
        view.offset += pointer - view.to_screen(canvas) * anchor;
        view
    }
}

struct NewLink {
    from: String,
    to: String,
    label: String,
    from_card: Card,
    to_card: Card,
}

impl Default for NewLink {
    fn default() -> Self {
        NewLink {
            from: String::new(),
            to: String::new(),
            label: String::new(),
            from_card: Card::ExactlyOne,
            to_card: Card::ZeroOrMore,
        }
    }
}

enum Export {
    Fitting {
        path: PathBuf,
        back: View,
    },
    Waiting {
        path: PathBuf,
        back: View,
        crop: Rect,
        image: Arc<Mutex<Option<Arc<ColorImage>>>>,
    },
}

#[derive(Default)]
struct App {
    source: Option<Source>,
    status: String,
    view: View,
    fit_pending: bool,
    dragging: Option<String>,
    show_links_panel: bool,
    new_link: NewLink,
    export: Option<Export>,
    session: Option<Session>,
    ticket: Option<String>,
    join_text: String,
    show_join: bool,
    peers: BTreeMap<String, (String, [f32; 2])>,
    last_pointer: Option<[f32; 2]>,
    unsaved_since: Option<Instant>,
    focus: Option<String>,
    arrange_pending: bool,
    before_arrange: Option<(String, BTreeMap<String, [f32; 2]>)>,
    stats: Stats,
    show_stats: bool,
    bench: Option<Bench>,
    motion: Option<Motion>,
    text_zoom: TextZoom,
}

impl App {
    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("{}: {e}", path.display());
                return;
            }
        };
        let diagrams = match parse_diagrams(&name, &text) {
            Ok(d) => d,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let layout = match layout::load(&path) {
            Ok(l) => l,
            Err(e) => {
                self.status = format!("layout not loaded, {e}");
                LayoutFile::default()
            }
        };
        let (tx, changes) = channel();
        let repaint = ctx.clone();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
            repaint.request_repaint();
        })
        .and_then(|mut w| {
            w.watch(
                path.parent().unwrap_or(Path::new(".")),
                RecursiveMode::NonRecursive,
            )?;
            Ok(w)
        });
        let watcher = match watcher {
            Ok(w) => Some(w),
            Err(e) => {
                self.status = format!("not watching for changes: {e}");
                None
            }
        };
        if !self.status.starts_with("not") && !self.status.starts_with("layout") {
            self.status.clear();
        }
        self.source = Some(Source {
            name,
            text,
            path: Some(path),
            diagrams,
            selected: 0,
            layout,
            _watcher: watcher,
            changes: Some(changes),
        });
        self.new_link = NewLink::default();
        self.fit_pending = true;
        self.send_source();
    }

    fn reload_if_changed(&mut self) {
        let Some(src) = &mut self.source else { return };
        let (Some(changes), Some(path)) = (&src.changes, src.path.clone()) else {
            return;
        };
        let mut changed = false;
        while let Ok(event) = changes.try_recv() {
            if let Ok(event) = event {
                changed |= !event.kind.is_access()
                    && event
                        .paths
                        .iter()
                        .any(|p| p.file_name() == path.file_name());
            }
        }
        if !changed {
            return;
        }
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|text| src.set_text(text));
        match result {
            Ok(()) => {
                self.status = format!("reloaded {}", path.display());
                self.send_source();
            }
            Err(e) => self.status = format!("reload failed, kept the last version: {e}"),
        }
    }

    fn send(&self, msg: Msg) {
        if let Some(s) = &self.session {
            s.send(msg);
        }
    }

    fn send_source(&self) {
        if let (Some(session), Some(src)) = (&self.session, &self.source)
            && session.hosting
        {
            session.send(Msg::Source {
                name: src.name.clone(),
                text: src.text.clone(),
            });
            session.send(Msg::Layout(src.layout.clone()));
        }
    }

    fn share(&mut self, ctx: &egui::Context) {
        self.session = Some(Session::host(ctx, self.stats.is_on()));
        self.status = "starting to share…".into();
    }

    fn join(&mut self, ctx: &egui::Context, ticket: &str) {
        match Session::join(ctx, ticket, self.stats.is_on()) {
            Ok(s) => {
                self.session = Some(s);
                self.source = None;
                self.show_join = false;
                self.join_text.clear();
            }
            Err(e) => self.status = e,
        }
    }

    fn stop_sharing(&mut self) {
        self.session = None;
        self.ticket = None;
        self.peers.clear();
        self.status = "sharing ended".into();
    }

    fn handle_session(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.session else { return };
        let events: Vec<Event> = session.events.try_iter().collect();
        for event in events {
            match event {
                Event::Ticket(t) => {
                    ctx.copy_text(t.clone());
                    self.ticket = Some(t);
                    self.status =
                        "sharing: the ticket is on the clipboard, send it to your peer".into();
                }
                Event::Status(s) => self.status = s,
                Event::Ended(s) => {
                    self.stop_sharing();
                    self.status = s;
                }
                Event::Joined(who) => {
                    self.status = format!("{who} joined");
                    self.send_source();
                }
                Event::Msg(msg) => self.apply(msg),
            }
        }
        if let Some(since) = self.unsaved_since {
            let wait = Duration::from_millis(500);
            if since.elapsed() >= wait {
                self.unsaved_since = None;
                self.save();
            } else {
                ctx.request_repaint_after(wait - since.elapsed());
            }
        }
    }

    fn apply(&mut self, msg: Msg) {
        match msg {
            Msg::Hello { .. } => {}
            Msg::Source { name, text } => {
                if self.source.is_none() {
                    self.fit_pending = true;
                    self.source = Some(Source {
                        name: name.clone(),
                        text: String::new(),
                        path: None,
                        diagrams: Vec::new(),
                        selected: 0,
                        layout: LayoutFile::default(),
                        _watcher: None,
                        changes: None,
                    });
                }
                let Some(src) = &mut self.source else { return };
                src.name = name;
                if let Err(e) = src.set_text(text) {
                    self.status = e;
                    self.source = None;
                }
            }
            Msg::Layout(layout) => {
                if let Some(src) = &mut self.source {
                    src.layout = layout;
                }
            }
            Msg::Move {
                diagram,
                table,
                pos,
            } => {
                if let Some(src) = &mut self.source {
                    src.layout
                        .diagrams
                        .entry(diagram)
                        .or_default()
                        .positions
                        .insert(table, pos);
                    self.unsaved_since.get_or_insert_with(Instant::now);
                }
            }
            Msg::Links { diagram, links } => {
                if let Some(src) = &mut self.source {
                    src.layout.diagrams.entry(diagram).or_default().links = links;
                    self.save();
                }
            }
            Msg::Pointer { who, diagram, pos } => match pos {
                Some(pos) => {
                    self.peers.insert(who, (diagram, pos));
                }
                None => {
                    self.peers.remove(&who);
                }
            },
        }
    }

    fn save(&mut self) {
        if let Some(src) = &self.source
            && let Err(e) = src.save()
        {
            self.status = format!("layout not saved: {e}");
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Mermaid or SQL", &["md", "markdown", "sql"])
                    .pick_file()
            {
                self.open(ui.ctx(), path);
            }
            match &self.session {
                None => {
                    if ui
                        .add_enabled(self.source.as_ref().is_some_and(|s| s.path.is_some()), egui::Button::new("Share"))
                        .on_hover_text("Let a peer join and edit this diagram with you")
                        .clicked()
                    {
                        self.share(ui.ctx());
                    }
                    ui.toggle_value(&mut self.show_join, "Join…");
                }
                Some(session) => {
                    if session.hosting
                        && let Some(ticket) = &self.ticket
                        && ui.button("Copy ticket").clicked()
                    {
                        ui.ctx().copy_text(ticket.clone());
                    }
                    let stop = if session.hosting { "Stop sharing" } else { "Leave" };
                    if ui.button(stop).clicked() {
                        self.stop_sharing();
                    }
                }
            }
            let Some(src) = &mut self.source else {
                if self.session.is_none() {
                    ui.label("Open a markdown file with a mermaid erDiagram block, or a Postgres schema .sql file.");
                }
                if !self.status.is_empty() {
                    ui.separator();
                    ui.label(&self.status);
                }
                return;
            };
            match &src.path {
                Some(path) => ui.label(path.display().to_string()),
                None => ui.label(format!("{} (shared by a peer)", src.name)),
            };
            if src.diagrams.len() > 1 {
                let before = src.selected;
                egui::ComboBox::from_id_salt("block")
                    .selected_text(src.title().to_string())
                    .show_ui(ui, |ui| {
                        for (i, (title, _)) in src.diagrams.iter().enumerate() {
                            ui.selectable_value(&mut src.selected, i, title);
                        }
                    });
                if src.selected != before {
                    self.fit_pending = true;
                }
            }
            if ui.button("Fit").clicked() {
                self.fit_pending = true;
            }
            if ui
                .button("Arrange")
                .on_hover_text("Lay the tables out in columns, with as few crossing lines as possible")
                .clicked()
            {
                self.arrange_pending = true;
            }
            if self.before_arrange.as_ref().is_some_and(|(t, _)| t == src.title()) && ui.button("Undo arrange").clicked() {
                let (title, positions) = self.before_arrange.take().unwrap();
                src.layout.diagrams.entry(title.clone()).or_default().positions = positions.clone();
                if let Some(s) = &self.session {
                    for (table, pos) in positions {
                        s.send(Msg::Move { diagram: title.clone(), table, pos });
                    }
                }
                if let Err(e) = src.save() {
                    self.status = format!("layout not saved: {e}");
                }
                self.fit_pending = true;
            }
            ui.toggle_value(&mut self.show_links_panel, "Hand-drawn links");
            if ui.button("Export PNG…").clicked() {
                let name = format!("{}.png", src.name);
                let mut dialog = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(name);
                if let Some(dir) = src.path.as_ref().and_then(|p| p.parent()) {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.save_file() {
                    self.export = Some(Export::Fitting { path, back: self.view });
                }
            }
            if !self.status.is_empty() {
                ui.separator();
                ui.label(&self.status);
            }
        });
    }

    fn links_panel(&mut self, ui: &mut egui::Ui) {
        let Some(src) = &mut self.source else { return };
        let names: Vec<String> = src
            .diagram()
            .tables
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let nl = &mut self.new_link;
        ui.heading("New link");
        egui::Grid::new("new_link").num_columns(2).show(ui, |ui| {
            for (label, value, card) in [
                ("From", &mut nl.from, &mut nl.from_card),
                ("To", &mut nl.to, &mut nl.to_card),
            ] {
                ui.label(label);
                egui::ComboBox::from_id_salt(label)
                    .selected_text(value.as_str())
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for n in &names {
                            ui.selectable_value(value, n.clone(), n);
                        }
                    });
                ui.end_row();
                ui.label("");
                egui::ComboBox::from_id_salt((label, "card"))
                    .selected_text(card.label())
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for c in Card::ALL {
                            ui.selectable_value(card, c, c.label());
                        }
                    });
                ui.end_row();
            }
            ui.label("Label");
            ui.text_edit_singleline(&mut nl.label);
            ui.end_row();
        });
        let ready = names.contains(&nl.from) && names.contains(&nl.to);
        let mut changed = false;
        if ui
            .add_enabled(ready, egui::Button::new("Add link"))
            .clicked()
        {
            src.diagram_layout().links.push(Link {
                from: nl.from.clone(),
                to: nl.to.clone(),
                label: nl.label.trim().to_string(),
                from_card: nl.from_card,
                to_card: nl.to_card,
            });
            nl.label.clear();
            changed = true;
        }
        ui.separator();
        ui.heading("Links drawn by hand");
        let links = &mut src.diagram_layout().links;
        if links.is_empty() {
            ui.weak("None yet.");
        }
        let mut remove = None;
        for (i, l) in links.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .small_button("✕")
                    .on_hover_text("Delete this link")
                    .clicked()
                {
                    remove = Some(i);
                }
                ui.label(format!("{} → {}: {}", l.from, l.to, l.label));
            });
        }
        if let Some(i) = remove {
            links.remove(i);
            changed = true;
        }
        if changed {
            let msg = Msg::Links {
                diagram: src.title().to_string(),
                links: src.diagram_layout().links.clone(),
            };
            self.send(msg);
            self.save();
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let shapes_before = self.stats.is_on().then(|| painted_shapes(&painter));
        let visuals = ui.visuals().clone();
        painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
        let Some(src) = &mut self.source else { return };

        let measure = |text: &str| {
            painter
                .layout_no_wrap(text.to_string(), FontId::proportional(FONT), Color32::WHITE)
                .size()
                .x
        };
        let sizes: Vec<Vec2> = self.stats.time("sizes", || {
            src.diagram()
                .tables
                .iter()
                .map(|t| box_size(t, &measure))
                .collect()
        });
        self.stats.time("placing", || place_new_tables(src, &sizes));

        let tables = src.diagram().tables.clone();
        if std::mem::take(&mut self.arrange_pending) {
            let edges = arrange::edges_of(&tables, src.links().map(|(l, _)| l));
            let placed = self
                .stats
                .time("arrange", || arrange::arrange(&sizes, &edges));
            let title = src.title().to_string();
            let positions = &mut src.diagram_layout().positions;
            self.before_arrange = Some((title.clone(), positions.clone()));
            for (t, p) in tables.iter().zip(placed) {
                positions.insert(t.name.clone(), [p.x, p.y]);
                if let Some(s) = &self.session {
                    s.send(Msg::Move {
                        diagram: title.clone(),
                        table: t.name.clone(),
                        pos: [p.x, p.y],
                    });
                }
            }
            if let Err(e) = src.save() {
                self.status = format!("layout not saved: {e}");
            }
            self.fit_pending = true;
        }
        let dl = src
            .layout
            .diagrams
            .get(src.title())
            .cloned()
            .unwrap_or_default();
        let world = world_rects(&tables, &sizes, &dl);

        if self.fit_pending || matches!(self.export, Some(Export::Fitting { .. })) {
            self.view = fit(&world, rect);
            self.fit_pending = false;
        }
        if let Some(motion) = self.motion.take() {
            self.view = apply_motion(motion, self.view, rect, src);
        }

        let to_world = self.view.to_world(rect.min);

        if let Some(pointer) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.0015).exp();
            if factor != 1.0 {
                self.view = self.view.zoomed(factor, rect.min, pointer);
            }
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let w = to_world * p;
            let hit = world
                .iter()
                .rposition(|r| r.contains(w))
                .map(|i| tables[i].name.clone());
            self.focus = if hit == self.focus { None } else { hit };
        }
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            let w = to_world * p;
            self.dragging = world
                .iter()
                .rposition(|r| r.contains(w))
                .map(|i| tables[i].name.clone());
        }
        if response.dragged() {
            let delta = response.drag_delta();
            match &self.dragging {
                Some(name) => {
                    let name = name.clone();
                    let zoom = self.view.zoom;
                    let title = src.title().to_string();
                    if let Some(p) = src.diagram_layout().positions.get_mut(&name) {
                        p[0] += delta.x / zoom;
                        p[1] += delta.y / zoom;
                        let msg = Msg::Move {
                            diagram: title,
                            table: name,
                            pos: *p,
                        };
                        if let Some(s) = &self.session {
                            s.send(msg);
                        }
                    }
                }
                None => self.view.offset += delta,
            }
        }
        if response.drag_stopped()
            && self.dragging.take().is_some()
            && let Err(e) = src.save()
        {
            self.status = format!("layout not saved: {e}");
        }

        let dl = src
            .layout
            .diagrams
            .get(src.title())
            .cloned()
            .unwrap_or_default();
        let world = world_rects(&tables, &sizes, &dl);
        let to_screen = self.view.to_screen(rect.min);
        let to_world = self.view.to_world(rect.min);
        let index = |name: &str| tables.iter().position(|t| t.name == name);

        let links: Vec<(Link, bool)> = src.links().map(|(l, h)| (l.clone(), h)).collect();
        let ends: Vec<Option<(usize, usize)>> = links
            .iter()
            .map(|(l, _)| Some((index(&l.from)?, index(&l.to)?)))
            .collect();
        let routes = self.stats.time("routing", || route_links(&ends, &world));
        let focus = self.focus.as_ref().filter(|f| index(f).is_some());
        let lit = |e: (usize, usize)| match focus {
            Some(f) => tables[e.0].name == *f || tables[e.1].name == *f,
            None => true,
        };
        let base = visuals.text_color();
        let now = Instant::now();
        let text_zoom = self
            .text_zoom
            .follow(self.view.zoom, now, self.export.is_some());
        if let Some(wait) = self.text_zoom.settles_in(now) {
            ui.ctx().request_repaint_after(wait);
        }
        let mut pen = Pen::new(&painter, to_screen, text_zoom);
        let mut labels: Vec<(&str, &Route)> = Vec::new();
        self.stats.time("links", || {
            for pass in [false, true] {
                for ((link, by_hand), (end, route)) in links.iter().zip(ends.iter().zip(&routes)) {
                    let (Some(end), Some(route)) = (end, route) else {
                        continue;
                    };
                    if lit(*end) != pass {
                        continue;
                    }
                    let color = match (focus, pass) {
                        (Some(_), true) => base.gamma_multiply(0.9),
                        (Some(_), false) => base.gamma_multiply(0.08),
                        (None, _) => base.gamma_multiply(0.55),
                    };
                    draw_route(&pen, link, *by_hand, route, color, &visuals);
                    if pass && !link.label.is_empty() {
                        labels.push((&link.label, route));
                    }
                }
            }
        });
        let show_labels = readable(LABEL_FONT * to_screen.scaling);
        if show_labels {
            let label_size = |text: &str| {
                let font = FontId::proportional(LABEL_FONT);
                let galley = painter.layout_no_wrap(text.to_string(), font, Color32::WHITE);
                galley.size() + Vec2::splat(4.0)
            };
            self.stats.time("labels", || {
                let spots = place_labels(&labels, &world, label_size);
                draw_labels(&mut pen, &labels, &spots, &visuals);
            });
        }

        let mut order: Vec<usize> = (0..tables.len()).collect();
        if let Some(d) = &self.dragging
            && let Some(i) = index(d)
        {
            order.retain(|&j| j != i);
            order.push(i);
        }
        self.stats.time("tables", || {
            for &i in &order {
                draw_table(&mut pen, &tables[i], world[i], &visuals);
            }
        });
        self.stats.time("text", || pen.finish());
        let hover = response.hover_pos().filter(|_| self.export.is_none());
        let tip = hover.and_then(|p| hover_tip(&tables, &world, &order, to_world * p));
        if let Some(tip) = tip
            && self.dragging.is_none()
        {
            response.clone().on_hover_text_at_pointer(tip);
        }

        let pointer = response.hover_pos().map(|p| (to_world * p).into());
        if self.session.is_some() && pointer != self.last_pointer {
            self.last_pointer = pointer;
            let msg = Msg::Pointer {
                who: share::who(),
                diagram: src.title().to_string(),
                pos: pointer,
            };
            if let Some(s) = &self.session {
                s.send(msg);
            }
        }
        if self.export.is_none() {
            for (who, (diagram, pos)) in &self.peers {
                if diagram == src.title() {
                    draw_peer(&painter, who, to_screen * Pos2::from(*pos));
                }
            }
        }
        if let Some(before) = shapes_before {
            let labels_shown = if show_labels { labels.len() } else { 0 };
            let counts = [
                ("tables", tables.len()),
                ("links", links.len()),
                ("routes", routes.iter().flatten().count()),
                ("labels", labels_shown),
                ("shapes", painted_shapes(&painter) - before),
            ];
            for (name, value) in counts {
                self.stats.count("scene", name, value);
            }
        }

        match self.export.take() {
            Some(Export::Fitting { path, back }) => {
                let image = Arc::new(Mutex::new(None));
                let slot = image.clone();
                let ctx = ui.ctx().clone();
                ui.ctx().request_screenshot(move |shot| {
                    *slot.lock().unwrap() = Some(shot);
                    ctx.request_repaint();
                });
                self.export = Some(Export::Waiting {
                    path,
                    back,
                    crop: rect,
                    image,
                });
            }
            Some(Export::Waiting {
                path,
                back,
                crop,
                image,
            }) => {
                let shot = image.lock().unwrap().take();
                match shot {
                    Some(shot) => {
                        self.status =
                            match save_png(&shot, crop, ui.ctx().pixels_per_point(), &path) {
                                Ok(()) => format!("saved {}", path.display()),
                                Err(e) => format!("PNG not saved: {e}"),
                            };
                        self.view = back;
                    }
                    None => {
                        self.export = Some(Export::Waiting {
                            path,
                            back,
                            crop,
                            image,
                        })
                    }
                }
            }
            None => {}
        }
    }

    fn drive_bench(&mut self, ctx: &egui::Context, cpu: Option<f32>) {
        let Some(mut bench) = self.bench.take() else {
            return;
        };
        bench.previous_frame_took(cpu);
        match bench.next_motion() {
            Some(motion) => {
                self.motion = Some(motion);
                self.bench = Some(bench);
                ctx.request_repaint();
            }
            None => {
                println!("{}", bench.report(&self.bench_subject(ctx)));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn bench_subject(&self, ctx: &egui::Context) -> String {
        let build = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let window = ctx.content_rect().size();
        let setup = format!(
            "{build} build, {}x pixels per point, window {:.0}×{:.0}",
            ctx.pixels_per_point(),
            window.x,
            window.y
        );
        let Some(src) = &self.source else {
            return format!("ferd bench: no diagram ({})\n{setup}", self.status);
        };
        format!(
            "ferd bench: {}, diagram \"{}\", {} tables, {} links\n{setup}",
            src.name,
            src.title(),
            src.diagram().tables.len(),
            src.links().count()
        )
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let cpu = frame.info().cpu_usage;
        self.stats.begin_frame(cpu);
        self.drive_bench(ui.ctx(), cpu);
        #[cfg(target_os = "macos")]
        for path in macos::opened(ui.ctx()) {
            self.open(ui.ctx(), path);
        }
        self.reload_if_changed();
        self.handle_session(ui.ctx());
        let exporting = self.export.is_some();
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_enabled_ui(!exporting, |ui| self.top_bar(ui));
        });
        if self.show_links_panel && self.source.is_some() {
            egui::Panel::right("links")
                .resizable(true)
                .default_size(300.0)
                .show(ui, |ui| self.links_panel(ui));
        }
        if self.show_join {
            let mut open = true;
            egui::Window::new("Join a shared diagram")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Paste the ticket your peer sent you:");
                    ui.text_edit_singleline(&mut self.join_text);
                    if ui
                        .add_enabled(!self.join_text.trim().is_empty(), egui::Button::new("Join"))
                        .clicked()
                    {
                        let ticket = self.join_text.clone();
                        self.join(ui.ctx(), &ticket);
                    }
                });
            self.show_join &= open;
        }
        let canvas = egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        if self.stats.is_on()
            && let Some(session) = &self.session
        {
            record_network(&mut self.stats, session);
        }
        if self.show_stats && self.export.is_none() {
            let started = Instant::now();
            self.stats.show(ui.ctx(), canvas.response.rect);
            self.stats.record("overlay", started.elapsed());
        }
        self.stats.end_frame();
        if let (Some(bench), Some(ui_ms)) = (&mut self.bench, self.stats.last_ui()) {
            bench.record(ui_ms, self.stats.frame_phases());
        }
    }
}

fn draw_peer(painter: &egui::Painter, who: &str, at: Pos2) {
    let color = Color32::from_rgb(230, 90, 160);
    let arrow = vec![
        at,
        at + vec2(0.0, 16.0),
        at + vec2(4.5, 12.0),
        at + vec2(11.0, 11.0),
    ];
    painter.add(Shape::convex_polygon(
        arrow,
        color,
        Stroke::new(1.0, Color32::WHITE),
    ));
    let galley =
        painter.layout_no_wrap(who.to_string(), FontId::proportional(11.0), Color32::WHITE);
    let r = Rect::from_min_size(at + vec2(12.0, 14.0), galley.size()).expand(3.0);
    painter.rect_filled(r, 3.0, color);
    painter.galley(r.min + Vec2::splat(3.0), galley, Color32::WHITE);
}

fn box_size(t: &Table, measure: &impl Fn(&str) -> f32) -> Vec2 {
    let name_w = t
        .columns
        .iter()
        .map(|c| measure(&c.name))
        .fold(0.0, f32::max);
    let type_w = t.columns.iter().map(|c| measure(&c.ty)).fold(0.0, f32::max);
    let rows_w = PAD + TAG_W + name_w + 18.0 + type_w + PAD;
    let header_w = PAD + measure(&t.name) * 1.1 + PAD;
    vec2(
        rows_w.max(header_w).max(140.0),
        HEADER_H + t.columns.len() as f32 * ROW_H + 6.0,
    )
}

fn place_new_tables(src: &mut Source, sizes: &[Vec2]) {
    let tables: Vec<String> = src
        .diagram()
        .tables
        .iter()
        .map(|t| t.name.clone())
        .collect();
    let placed_before = src
        .layout
        .diagrams
        .get(src.title())
        .is_some_and(|dl| tables.iter().any(|t| dl.positions.contains_key(t)));
    if !placed_before {
        arrange_everything(src, &tables, sizes);
        return;
    }
    let cell = sizes.iter().fold(Vec2::ZERO, |a, s| a.max(*s)) + Vec2::splat(GAP);
    let cols = (tables.len() as f32).sqrt().ceil().max(1.0) as usize;
    let dl = src.diagram_layout();
    let size_of = |name: &str| sizes[tables.iter().position(|t| t == name).unwrap()];
    for (name, size) in tables.iter().zip(sizes) {
        if dl.positions.contains_key(name) {
            continue;
        }
        let taken: Vec<Rect> = dl
            .positions
            .iter()
            .filter(|(n, _)| tables.contains(n))
            .map(|(n, p)| Rect::from_min_size(Pos2::from(*p), size_of(n)).expand(GAP / 2.0 - 1.0))
            .collect();
        let spot = (0..)
            .map(|i| pos2((i % cols) as f32 * cell.x, (i / cols) as f32 * cell.y))
            .find(|p| {
                !taken
                    .iter()
                    .any(|r| r.intersects(Rect::from_min_size(*p, *size)))
            })
            .unwrap();
        dl.positions.insert(name.clone(), [spot.x, spot.y]);
    }
}

fn arrange_everything(src: &mut Source, tables: &[String], sizes: &[Vec2]) {
    let edges = arrange::edges_of(&src.diagram().tables, src.links().map(|(l, _)| l));
    let placed = arrange::arrange(sizes, &edges);
    let positions = &mut src.diagram_layout().positions;
    for (name, p) in tables.iter().zip(placed) {
        positions.insert(name.clone(), [p.x, p.y]);
    }
}

fn record_network(stats: &mut Stats, session: &Session) {
    let Some(traffic) = session.traffic() else {
        return;
    };
    let (sent, received) = (
        Tally::total(traffic.sent.values()),
        Tally::total(traffic.received.values()),
    );
    let wires = &traffic.wires;
    let direct = wires.iter().filter(|w| w.direct).count();
    let slowest = wires.iter().filter_map(|w| w.rtt).max();
    let flow = traffic.per_second;
    let role = if session.hosting { "host" } else { "guest" };
    let readings = [
        ("role", Value::Text(role.into())),
        ("peers", Value::Count(wires.len() as u64)),
        ("msgs out/s", Value::Count(flow.messages_out.round() as u64)),
        ("msgs in/s", Value::Count(flow.messages_in.round() as u64)),
        ("wire out/s", Value::Bytes(flow.wire_out)),
        ("wire in/s", Value::Bytes(flow.wire_in)),
        ("sent", Value::Bytes(sent.bytes as f64)),
        ("received", Value::Bytes(received.bytes as f64)),
        (
            "direct",
            Value::Text(format!("{direct} of {}", wires.len())),
        ),
        (
            "lost",
            Value::Count(wires.iter().map(|w| w.lost_packets).sum()),
        ),
    ];
    for (name, value) in readings {
        stats.set("network", name, value);
    }
    if let Some(rtt) = slowest {
        stats.set("network", "rtt", Value::Millis(rtt.as_secs_f32() * 1000.0));
    }
    let mut kinds: Vec<&'static str> = traffic.sent.keys().copied().collect();
    kinds.extend(
        traffic
            .received
            .keys()
            .filter(|k| !traffic.sent.contains_key(*k)),
    );
    for kind in kinds {
        let out = traffic.sent.get(kind).map_or(0, |t| t.messages);
        let came = traffic.received.get(kind).map_or(0, |t| t.messages);
        stats.set(
            "messages out/in",
            kind,
            Value::Text(format!("{out}/{came}")),
        );
    }
}

fn painted_shapes(painter: &egui::Painter) -> usize {
    let layer = painter.layer_id();
    painter
        .ctx()
        .graphics(|g| g.get(layer).map_or(0, |shapes| shapes.all_entries().len()))
}

fn apply_motion(motion: Motion, view: View, canvas: Rect, src: &mut Source) -> View {
    match motion {
        Motion::Still => view,
        Motion::Zoom(factor) => view.zoomed(factor, canvas.min, canvas.center()),
        Motion::Pan(by) => View {
            offset: view.offset + by,
            ..view
        },
        Motion::Drag(by) => {
            if let Some(name) = busiest_table(src)
                && let Some(p) = src.diagram_layout().positions.get_mut(&name)
            {
                p[0] += by.x;
                p[1] += by.y;
            }
            view
        }
    }
}

fn busiest_table(src: &Source) -> Option<String> {
    let mut degree: BTreeMap<&str, usize> = BTreeMap::new();
    for (link, _) in src.links() {
        *degree.entry(&link.from).or_default() += 1;
        *degree.entry(&link.to).or_default() += 1;
    }
    let busiest = degree.into_iter().max_by_key(|&(_, d)| d)?;
    Some(busiest.0.to_string())
}

fn world_rects(tables: &[Table], sizes: &[Vec2], dl: &DiagramLayout) -> Vec<Rect> {
    tables
        .iter()
        .zip(sizes)
        .map(|(t, s)| Rect::from_min_size(Pos2::from(dl.positions[&t.name]), *s))
        .collect()
}

fn fit(world: &[Rect], screen: Rect) -> View {
    let Some(bounds) = world.iter().copied().reduce(|a, b| a.union(b)) else {
        return View::default();
    };
    let bounds = bounds.expand(30.0);
    let zoom = (screen.width() / bounds.width())
        .min(screen.height() / bounds.height())
        .clamp(MIN_ZOOM, MAX_FIT_ZOOM);
    let offset = (screen.size() - bounds.size() * zoom) / 2.0 - bounds.min.to_vec2() * zoom;
    View { offset, zoom }
}

fn readable(font: f32) -> bool {
    font >= SMALLEST_FONT
}

fn header_rect(table: Rect) -> Rect {
    Rect::from_min_size(table.min, vec2(table.width(), HEADER_H))
}

fn row_rect(table: Rect, i: usize) -> Rect {
    let top = table.min.y + HEADER_H + i as f32 * ROW_H;
    Rect::from_min_max(pos2(table.min.x, top), pos2(table.max.x, top + ROW_H))
}

fn draw_table(pen: &mut Pen, t: &Table, world: Rect, visuals: &egui::Visuals) {
    let border = visuals
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(0.6);
    let radius = 5.0;
    let header_corners = CornerRadiusF32 {
        nw: radius,
        ne: radius,
        sw: 0.0,
        se: 0.0,
    };
    let header_fill = visuals.selection.bg_fill.gamma_multiply(0.45);
    pen.rect(world, radius, visuals.panel_fill, Stroke::NONE);
    pen.rect(
        header_rect(world),
        header_corners,
        header_fill,
        Stroke::NONE,
    );
    pen.rect(
        world,
        radius,
        Color32::TRANSPARENT,
        Stroke::new(1.0, border),
    );

    if !readable(FONT * pen.zoom()) {
        return;
    }
    let text = visuals.text_color();
    pen.text(
        pos2(world.min.x + PAD, header_rect(world).center().y),
        Align2::LEFT_CENTER,
        &t.name,
        FontId::proportional(FONT * 1.1),
        text,
    );
    for (i, c) in t.columns.iter().enumerate() {
        let y = row_rect(world, i).center().y + 1.0;
        if let Some((tag, color)) = key_tag(c) {
            let font = FontId::monospace(FONT * 0.8);
            pen.text(
                pos2(world.min.x + PAD, y),
                Align2::LEFT_CENTER,
                tag,
                font,
                color,
            );
        }
        let name = if c.note.is_some() {
            format!("{} ·", c.name)
        } else {
            c.name.clone()
        };
        pen.text(
            pos2(world.min.x + PAD + TAG_W, y),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(FONT),
            text,
        );
        pen.text(
            pos2(world.max.x - PAD, y),
            Align2::RIGHT_CENTER,
            &c.ty,
            FontId::proportional(FONT),
            visuals.weak_text_color(),
        );
    }
}

fn key_tag(c: &Column) -> Option<(&'static str, Color32)> {
    let primary = Color32::from_rgb(214, 160, 40);
    let foreign = Color32::from_rgb(90, 150, 220);
    match (c.pk, c.fk) {
        (true, true) => Some(("PF", primary)),
        (true, false) => Some(("PK", primary)),
        (false, true) => Some(("FK", foreign)),
        (false, false) => None,
    }
}

fn hover_tip(tables: &[Table], world: &[Rect], order: &[usize], at: Pos2) -> Option<String> {
    let top = *order.iter().rev().find(|&&i| world[i].contains(at))?;
    table_tip(&tables[top], world[top], at)
}

fn table_tip(t: &Table, world: Rect, at: Pos2) -> Option<String> {
    let row = (0..t.columns.len())
        .rev()
        .find(|&i| row_rect(world, i).contains(at));
    match row {
        Some(i) => Some(column_tip(&t.columns[i])),
        None if header_rect(world).contains(at) => t.note.clone(),
        None => None,
    }
}

fn column_tip(c: &Column) -> String {
    let mut lines = vec![format!("{} {}", c.name, c.ty)];
    lines.extend(c.note.clone());
    lines.join("\n")
}

fn draw_route(
    pen: &Pen,
    link: &Link,
    by_hand: bool,
    route: &Route,
    color: Color32,
    visuals: &egui::Visuals,
) {
    let stroke = Stroke::new(1.4, color);
    if by_hand {
        pen.dashed_line(&route.pts, stroke, 7.0, 5.0);
    } else {
        pen.line(&route.pts, stroke);
    }
    let last = route.pts.len() - 1;
    let ends = [
        (route.pts[0], route.from_u, link.from_card),
        (route.pts[last], route.to_u, link.to_card),
    ];
    for (at, out, card) in ends {
        mark(pen, at, out, card, stroke, visuals);
    }
}

fn mark(pen: &Pen, p: Pos2, u: Vec2, card: Card, stroke: Stroke, visuals: &egui::Visuals) {
    let n = vec2(-u.y, u.x);
    let at = |along: f32, across: f32| p + u * along + n * across;
    let bar = |d: f32| pen.segment([at(d, 6.0), at(d, -6.0)], stroke);
    let ring = |d: f32| pen.circle(at(d, 0.0), 4.0, visuals.extreme_bg_color, stroke);
    let foot = || {
        let tip = at(11.0, 0.0);
        pen.segment([tip, at(0.0, 7.0)], stroke);
        pen.segment([tip, at(0.0, -7.0)], stroke);
    };
    match card {
        Card::ExactlyOne => {
            bar(8.0);
            bar(13.0);
        }
        Card::ZeroOrOne => {
            bar(8.0);
            ring(17.0);
        }
        Card::OneOrMore => {
            foot();
            bar(15.0);
        }
        Card::ZeroOrMore => {
            foot();
            ring(17.0);
        }
    }
}

fn place_labels(
    labels: &[(&str, &Route)],
    boxes: &[Rect],
    size_of: impl Fn(&str) -> Vec2,
) -> Vec<Rect> {
    let mut spots: Vec<Rect> = Vec::with_capacity(labels.len());
    for (text, route) in labels {
        let spot = label_spot(&route.pts, size_of(text), boxes, &spots);
        spots.push(spot);
    }
    spots
}

fn draw_labels(pen: &mut Pen, labels: &[(&str, &Route)], spots: &[Rect], visuals: &egui::Visuals) {
    let fill = visuals.extreme_bg_color.gamma_multiply(0.9);
    for ((text, _), spot) in labels.iter().zip(spots) {
        pen.rect(*spot, 3.0, fill, Stroke::NONE);
        pen.text(
            spot.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(LABEL_FONT),
            visuals.text_color(),
        );
    }
}

fn label_spot(pts: &[Pos2], size: Vec2, boxes: &[Rect], taken: &[Rect]) -> Rect {
    let inner = if pts.len() > 2 {
        &pts[1..pts.len() - 1]
    } else {
        pts
    };
    let mut segments: Vec<(Pos2, Pos2)> = inner.windows(2).map(|w| (w[0], w[1])).collect();
    if segments.is_empty() {
        segments.push((pts[0], pts[pts.len() - 1]));
    }
    segments.sort_by(|a, b| (b.1 - b.0).length().total_cmp(&(a.1 - a.0).length()));
    let at =
        |(u, v): (Pos2, Pos2), t: f32| Align2::CENTER_CENTER.anchor_size(u + (v - u) * t, size);
    let mut best = (usize::MAX, at(segments[0], 0.5));
    for &seg in &segments {
        for t in [0.5, 0.35, 0.65, 0.2, 0.8, 0.1, 0.9] {
            let r = at(seg, t);
            let hits = taken.iter().filter(|o| o.intersects(r)).count() * 10
                + boxes.iter().filter(|o| o.intersects(r)).count();
            if hits == 0 {
                return r;
            }
            if hits < best.0 {
                best = (hits, r);
            }
        }
    }
    best.1
}

fn icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("assets/icon.png is a valid PNG")
        .into_rgba8();
    egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    }
}

fn save_png(shot: &ColorImage, crop: Rect, ppp: f32, path: &Path) -> Result<(), String> {
    let region = shot.region(&crop, Some(ppp));
    let [w, h] = region.size;
    let bytes: Vec<u8> = region.pixels.iter().flat_map(|c| c.to_array()).collect();
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Result<Args, String> {
        parse_args(words.iter().map(OsString::from))
    }

    #[test]
    fn options_go_anywhere() {
        let expected = Args {
            path: Some(PathBuf::from("shop.md")),
            stats: true,
            bench: true,
            ..Args::default()
        };
        assert_eq!(args(&["--stats", "--bench", "shop.md"]), Ok(expected));
        let stats_last = args(&["shop.md", "--stats"]).unwrap();
        assert!(stats_last.stats && stats_last.path.is_some());
        let joining = args(&["--join", "ferd00", "--stats"]).unwrap();
        assert_eq!(joining.join.as_deref(), Some("ferd00"));
        assert!(joining.stats);
        assert_eq!(args(&[]), Ok(Args::default()));
    }

    #[test]
    fn bad_arguments_are_refused() {
        for words in [
            &["--join"][..],
            &["--fast", "shop.md"],
            &["a.md", "b.md"],
            &["a.md", "--join", "ferd00"],
            &["--bench"],
            &["--bench", "--join", "ferd00"],
        ] {
            assert!(args(words).is_err(), "{words:?}");
        }
    }

    #[test]
    fn new_diagrams_are_arranged_and_new_tables_find_a_free_spot() {
        let mut src = source_of("```mermaid\nerDiagram\n a ||--o{ b : x\n c }o--|| b : y\n```\n");
        let size = vec2(100.0, 50.0);
        place_new_tables(&mut src, &[size; 3]);
        let edges = arrange::edges_of(&src.diagram().tables, &src.diagram().links);
        let arranged = arrange::arrange(&[size; 3], &edges);
        let before = src.diagram_layout().positions.clone();
        for (table, p) in src.diagram().tables.iter().zip(arranged) {
            assert_eq!(before[&table.name], [p.x, p.y], "{}", table.name);
        }

        let d = Table {
            name: "d".into(),
            ..Table::default()
        };
        src.diagrams[0].1.tables.push(d);
        place_new_tables(&mut src, &[size; 4]);
        let after = &src.diagram_layout().positions;
        let spot = |name: &str| Rect::from_min_size(Pos2::from(after[name]), size);
        for name in ["a", "b", "c"] {
            assert_eq!(after[name], before[name], "{name} moved");
            assert!(!spot("d").intersects(spot(name)), "d is on {name}");
        }
    }

    #[test]
    fn only_the_table_on_top_gives_a_tip() {
        let id = Column {
            name: "id".into(),
            ty: "int".into(),
            ..Column::default()
        };
        let under = Table {
            name: "under".into(),
            columns: vec![id],
            ..Table::default()
        };
        let over = Table {
            name: "over".into(),
            ..Table::default()
        };
        let tables = [under, over];
        let world = [
            Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 80.0)),
            Rect::from_min_size(pos2(0.0, 20.0), vec2(200.0, 80.0)),
        ];
        let on_under_row_and_over_header = pos2(50.0, 30.0);
        let tip = |order: &[usize]| hover_tip(&tables, &world, order, on_under_row_and_over_header);
        assert_eq!(tip(&[0, 1]), None);
        assert_eq!(tip(&[1, 0]).as_deref(), Some("id int"));
    }

    #[test]
    fn table_corners_follow_the_zoom_and_match_the_header() {
        let ctx = egui::Context::default();
        ctx.set_fonts(canvas_fonts());
        let table = Table {
            name: "t".into(),
            ..Table::default()
        };
        let world = Rect::from_min_size(Pos2::ZERO, vec2(150.0, 40.0));
        for zoom in [0.5, 1.1, 1.3, 2.0, 3.7] {
            let mut out = ctx.run_pass(Default::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let mut pen = Pen::new(&painter, TSTransform::new(Vec2::ZERO, zoom), zoom);
                draw_table(&mut pen, &table, world, &ctx.global_style().visuals);
                pen.finish();
            });
            out.textures_delta.clear();
            let rects: Vec<&egui::epaint::RectShape> = out
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    Shape::Rect(rect) => Some(rect),
                    _ => None,
                })
                .collect();
            let [body, header, border] = rects[..] else {
                panic!("a table is three rects, got {}", rects.len());
            };
            let radius = (5.0 * zoom).round() as u8;
            assert_eq!(body.corner_radius.nw, radius, "at zoom {zoom}");
            assert_eq!(header.corner_radius.nw, radius, "at zoom {zoom}");
            assert_eq!(header.corner_radius.sw, 0, "at zoom {zoom}");
            assert_eq!(border.corner_radius, body.corner_radius, "at zoom {zoom}");
        }
    }

    #[test]
    fn labels_on_the_same_segment_do_not_overlap() {
        let pts = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 300.0),
            pos2(20.0, 300.0),
        ];
        let size = vec2(80.0, 16.0);
        let a = label_spot(&pts, size, &[], &[]);
        let b = label_spot(&pts, size, &[], &[a]);
        assert!(!a.intersects(b));
    }

    #[test]
    fn every_view_maps_the_screen_back_onto_the_diagram() {
        let (ends, world) = planned_diagram();
        let points: Vec<Pos2> = route_links(&ends, &world)
            .into_iter()
            .flatten()
            .flat_map(|route| route.pts)
            .collect();
        let canvas = pos2(220.0, 30.0);
        for view in zoom_and_pan_views() {
            let (to_screen, to_world) = (view.to_screen(canvas), view.to_world(canvas));
            for &p in &points {
                let back = to_world * (to_screen * p);
                assert!(back.distance(p) < 0.01, "{p:?} came back as {back:?}");
            }
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let canvas = pos2(220.0, 30.0);
        let pointer = pos2(613.4, 377.9);
        for view in zoom_and_pan_views() {
            for factor in [0.5, 0.97, 1.03, 2.0] {
                let under = view.to_world(canvas) * pointer;
                let zoomed = view.zoomed(factor, canvas, pointer);
                let after = zoomed.to_screen(canvas) * under;
                assert!(after.distance(pointer) < 0.01, "{after:?} at {factor}");
                assert!((MIN_ZOOM..=MAX_ZOOM).contains(&zoomed.zoom));
            }
        }
    }

    #[test]
    fn every_bench_zoom_frame_zooms_from_any_fitted_view() {
        let (canvas, centre) = (pos2(220.0, 30.0), pos2(860.0, 460.0));
        for start in [MIN_ZOOM, MAX_FIT_ZOOM] {
            let mut view = View {
                offset: Vec2::ZERO,
                zoom: start,
            };
            let mut bench = Bench::default();
            while let Some(motion) = bench.next_motion() {
                let Motion::Zoom(factor) = motion else {
                    continue;
                };
                let wanted = view.zoom * factor;
                view = view.zoomed(factor, canvas, centre);
                let off = (view.zoom / wanted - 1.0).abs();
                assert!(off < 1e-4, "{wanted} became {} from {start}", view.zoom);
            }
            let back = (view.zoom / start - 1.0).abs();
            assert!(back < 1e-3, "{start} came back as {}", view.zoom);
        }
    }

    fn source_of(markdown: &str) -> Source {
        Source {
            name: "test.md".into(),
            text: markdown.into(),
            path: None,
            diagrams: parse_diagrams("test.md", markdown).unwrap(),
            selected: 0,
            layout: LayoutFile::default(),
            _watcher: None,
            changes: None,
        }
    }

    fn zoom_and_pan_views() -> Vec<View> {
        let mut views = Vec::new();
        let mut zoom = 0.25;
        while zoom < 4.0 {
            for offset in [vec2(0.0, 0.0), vec2(13.37, -7.21), vec2(-250.5, 410.25)] {
                views.push(View { offset, zoom });
            }
            zoom *= 1.01;
        }
        views
    }

    fn planned_diagram() -> (Vec<Option<(usize, usize)>>, Vec<Rect>) {
        let blocks = mermaid::blocks(include_str!("../tests/fixtures/library.md"));
        let planned = &blocks
            .iter()
            .find(|b| b.title == "Planned")
            .unwrap()
            .diagram;
        let measure = |text: &str| text.len() as f32 * 7.0;
        let sizes: Vec<Vec2> = planned
            .tables
            .iter()
            .map(|t| box_size(t, &measure))
            .collect();
        let index = |name: &str| planned.tables.iter().position(|t| t.name == name);
        let ends: Vec<Option<(usize, usize)>> = planned
            .links
            .iter()
            .map(|l| Some((index(&l.from)?, index(&l.to)?)))
            .collect();
        let edges = arrange::edges_of(&planned.tables, &planned.links);
        let world = arrange::arrange(&sizes, &edges)
            .into_iter()
            .zip(&sizes)
            .map(|(p, s)| Rect::from_min_size(p, *s))
            .collect();
        (ends, world)
    }
}
