mod arrange;
mod layout;
#[cfg(target_os = "macos")]
mod macos;
mod mermaid;
mod model;
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
use eframe::egui::epaint::{CornerRadiusF32, RectShape};
use eframe::egui::layers::ShapeIdx;
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
    pos2, vec2,
};
use notify::{RecursiveMode, Watcher};

use layout::{DiagramLayout, LayoutFile};
use model::{Card, Column, Diagram, Link, Table};
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
const TEXT_SETTLES: Duration = Duration::from_millis(150);
const TEXT_AHEAD: f32 = 1.5;
const CANVAS_FONT: &str = "canvas";
const CANVAS_MONO: &str = "canvas mono";
const SAME_COST: f32 = 0.5;
const SIDE_BY_SIDE: f32 = 40.0;
const LANE: f32 = 9.0;

const USAGE: &str = "usage: ferd [--stats] [file | --join ticket]";

#[derive(Debug, Default, PartialEq)]
struct Args {
    path: Option<PathBuf>,
    join: Option<String>,
    stats: bool,
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--stats") => parsed.stats = true,
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
                stats: Stats::new(args.stats),
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
            let index = |name: &str| tables.iter().position(|t| t.name == name);
            let edges: Vec<(usize, usize)> = src
                .links()
                .filter_map(|(l, _)| Some((index(&l.from)?, index(&l.to)?)))
                .collect();
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
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let cpu = frame.info().cpu_usage;
        self.stats.begin_frame(cpu);
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
        if self.stats.is_on() && self.export.is_none() {
            let started = Instant::now();
            self.stats.show(ui.ctx(), canvas.response.rect);
            self.stats.record("overlay", started.elapsed());
        }
        self.stats.end_frame();
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
    let index = |name: &str| tables.iter().position(|t| t == name);
    let edges: Vec<(usize, usize)> = src
        .links()
        .filter_map(|(l, _)| Some((index(&l.from)?, index(&l.to)?)))
        .collect();
    let cell = sizes.iter().fold(Vec2::ZERO, |a, s| a.max(*s)) + Vec2::splat(GAP);
    let cols = (tables.len() as f32).sqrt().ceil().max(1.0) as usize;
    let dl = src.diagram_layout();
    if !tables.iter().any(|t| dl.positions.contains_key(t)) {
        for (name, p) in tables.iter().zip(arrange::arrange(sizes, &edges)) {
            dl.positions.insert(name.clone(), [p.x, p.y]);
        }
        return;
    }
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

struct Pen<'a> {
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
    fn new(painter: &'a egui::Painter, to_screen: TSTransform, text_zoom: f32) -> Pen<'a> {
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

    fn zoom(&self) -> f32 {
        self.to_screen.scaling
    }

    fn screen(&self, world: &[Pos2]) -> Vec<Pos2> {
        world.iter().map(|&p| self.to_screen * p).collect()
    }

    fn rect(&self, world: Rect, corners: impl Into<CornerRadiusF32>, fill: Color32, line: Stroke) {
        let screen = self.to_screen * world;
        let corners = corners.into() * self.zoom();
        let shape = RectShape::new(screen, corners, fill, line, StrokeKind::Inside);
        self.painter.add(shape.with_round_to_pixels(false));
    }

    fn line(&self, world: &[Pos2], line: Stroke) {
        self.painter.add(Shape::line(self.screen(world), line));
    }

    fn dashed_line(&self, world: &[Pos2], line: Stroke, dash: f32, gap: f32) {
        let (dash, gap) = (dash * self.zoom(), gap * self.zoom());
        let dashes = Shape::dashed_line(&self.screen(world), line, dash, gap);
        self.painter.extend(dashes);
    }

    fn segment(&self, world: [Pos2; 2], line: Stroke) {
        let [a, b] = world;
        self.painter
            .line_segment([self.to_screen * a, self.to_screen * b], line);
    }

    fn circle(&self, center: Pos2, radius: f32, fill: Color32, line: Stroke) {
        let (center, radius) = (self.to_screen * center, radius * self.zoom());
        self.painter.circle(center, radius, fill, line);
    }

    fn text(&mut self, at: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
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

    fn finish(self) {
        let layer = self.painter.layer_id();
        self.painter.ctx().graphics_mut(|g| {
            let shapes = g.entry(layer);
            for (shape, transform) in self.texts {
                shapes.mutate_shape(shape, |s| s.transform_after_tessellation = transform);
            }
        });
    }
}

fn canvas_fonts() -> egui::FontDefinitions {
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
struct TextZoom {
    current: f32,
    last_zoom: f32,
    moved_at: Option<Instant>,
}

impl TextZoom {
    fn follow(&mut self, zoom: f32, now: Instant, exporting: bool) -> f32 {
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

    fn settles_in(&self, now: Instant) -> Option<Duration> {
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn out(self) -> Vec2 {
        match self {
            Side::Left => vec2(-1.0, 0.0),
            Side::Right => vec2(1.0, 0.0),
            Side::Top => vec2(0.0, -1.0),
            Side::Bottom => vec2(0.0, 1.0),
        }
    }

    fn vertical(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

type Slots = std::collections::HashMap<(usize, Side), Vec<(usize, usize, f32)>>;

struct Route {
    pts: Vec<Pos2>,
    from_u: Vec2,
    to_u: Vec2,
}

fn sides(a: Rect, b: Rect) -> (Side, Side) {
    let room = 40.0;
    if a.max.x + room <= b.min.x {
        (Side::Right, Side::Left)
    } else if b.max.x + room <= a.min.x {
        (Side::Left, Side::Right)
    } else if a.max.y + room <= b.min.y {
        (Side::Bottom, Side::Top)
    } else if b.max.y + room <= a.min.y {
        (Side::Top, Side::Bottom)
    } else {
        (Side::Right, Side::Right)
    }
}

fn route_links(ends: &[Option<(usize, usize)>], boxes: &[Rect]) -> Vec<Option<Route>> {
    let mut slots: Slots = Default::default();
    let mut chosen = vec![None; ends.len()];
    for (i, end) in ends.iter().enumerate() {
        let Some((a, b)) = *end else { continue };
        let (sa, sb) = if a == b {
            (Side::Right, Side::Right)
        } else {
            sides(boxes[a], boxes[b])
        };
        chosen[i] = Some((sa, sb));
        let along = |r: Rect, s: Side| {
            if s.vertical() {
                r.center().y
            } else {
                r.center().x
            }
        };
        slots
            .entry((a, sa))
            .or_default()
            .push((i, 0, along(boxes[b], sa)));
        slots.entry((b, sb)).or_default().push((
            i,
            1,
            along(boxes[a], sb) + if a == b { 1.0 } else { 0.0 },
        ));
    }
    let mut at = vec![[Pos2::ZERO; 2]; ends.len()];
    for ((bx, side), list) in &mut slots {
        list.sort_by(|x, y| x.2.total_cmp(&y.2).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));
        let r = boxes[*bx];
        let n = list.len() as f32;
        for (k, &(i, end, _)) in list.iter().enumerate() {
            let t = (k as f32 + 1.0) / (n + 1.0);
            at[i][end] = match side {
                Side::Left | Side::Right => {
                    let top = (r.min.y + HEADER_H).min(r.max.y);
                    let y = top + (r.max.y - top) * t;
                    pos2(
                        if *side == Side::Left {
                            r.min.x
                        } else {
                            r.max.x
                        },
                        y,
                    )
                }
                Side::Top | Side::Bottom => {
                    let x = r.min.x + r.width() * t;
                    pos2(x, if *side == Side::Top { r.min.y } else { r.max.y })
                }
            };
        }
    }
    let mut routes: Vec<Option<Route>> = Vec::with_capacity(ends.len());
    for (i, end) in ends.iter().enumerate() {
        let (Some((a, b)), Some((sa, sb))) = (*end, chosen[i]) else {
            routes.push(None);
            continue;
        };
        let (p, q) = (at[i][0], at[i][1]);
        let obstacles: Vec<Rect> = boxes
            .iter()
            .enumerate()
            .filter(|&(k, _)| k != a && k != b)
            .map(|(_, r)| r.expand(4.0))
            .collect();
        let pts = match (sa, sb) {
            (Side::Right, Side::Right) => {
                let x = boxes[a].max.x.max(boxes[b].max.x) + 30.0;
                vec![p, pos2(x, p.y), pos2(x, q.y), q]
            }
            (s, _) if s.vertical() => best_path(p, q, &obstacles),
            _ => {
                let t = |p: Pos2| pos2(p.y, p.x);
                let flipped: Vec<Rect> = obstacles
                    .iter()
                    .map(|r| Rect::from_min_max(t(r.min), t(r.max)))
                    .collect();
                best_path(t(p), t(q), &flipped).into_iter().map(t).collect()
            }
        };
        routes.push(Some(Route {
            pts,
            from_u: sa.out(),
            to_u: sb.out(),
        }));
    }
    spread_side_by_side(&mut routes);
    routes
}

struct Middle {
    route: usize,
    vertical: bool,
    at: f32,
}

fn middle(route: usize, pts: &[Pos2]) -> Option<Middle> {
    if pts.len() != 4 {
        return None;
    }
    let vertical = pts[1].x == pts[2].x;
    let at = if vertical { pts[1].x } else { pts[1].y };
    Some(Middle {
        route,
        vertical,
        at,
    })
}

fn spread_side_by_side(routes: &mut [Option<Route>]) {
    let mut middles: Vec<Middle> = routes
        .iter()
        .enumerate()
        .filter_map(|(i, route)| middle(i, &route.as_ref()?.pts))
        .collect();
    middles.sort_by(|a, b| a.vertical.cmp(&b.vertical).then(a.at.total_cmp(&b.at)));
    let side_by_side =
        |a: &Middle, b: &Middle| a.vertical == b.vertical && b.at - a.at < SIDE_BY_SIDE;
    for group in middles.chunk_by(side_by_side) {
        spread(routes, group);
    }
}

fn spread(routes: &mut [Option<Route>], group: &[Middle]) {
    let mut members: Vec<usize> = group.iter().map(|m| m.route).collect();
    // Link order, not position: a line passing another keeps its lane instead of both jumping.
    members.sort();
    let n = members.len() as f32;
    for (k, i) in members.into_iter().enumerate() {
        let shift = (k as f32 - (n - 1.0) / 2.0) * LANE;
        if let Some(route) = &mut routes[i] {
            for p in &mut route.pts[1..3] {
                if group[0].vertical {
                    p.x += shift;
                } else {
                    p.y += shift;
                }
            }
        }
    }
}

fn best_path(p: Pos2, q: Pos2, obstacles: &[Rect]) -> Vec<Pos2> {
    let candidates = candidate_paths(p, q, obstacles);
    let fewest_hits = candidates
        .iter()
        .map(|path| obstacle_hits(path, obstacles))
        .min()
        .unwrap();
    let least_blocked: Vec<Vec<Pos2>> = candidates
        .into_iter()
        .filter(|path| obstacle_hits(path, obstacles) == fewest_hits)
        .collect();
    let cheapest = least_blocked
        .iter()
        .map(|path| path_cost(path))
        .fold(f32::INFINITY, f32::min);
    least_blocked
        .into_iter()
        .find(|path| path_cost(path) < cheapest + SAME_COST)
        .unwrap()
}

fn candidate_paths(p: Pos2, q: Pos2, obstacles: &[Rect]) -> Vec<Vec<Pos2>> {
    let d = if q.x >= p.x { 1.0 } else { -1.0 };
    let (x1, x2) = (p.x + d * 22.0, q.x - d * 22.0);
    let mut candidates: Vec<Vec<Pos2>> = [(p.x + q.x) / 2.0, x1, x2]
        .into_iter()
        .map(|x| vec![p, pos2(x, p.y), pos2(x, q.y), q])
        .collect();
    let (lo, hi) = (p.x.min(q.x), p.x.max(q.x));
    for r in obstacles.iter().filter(|r| r.max.x > lo && r.min.x < hi) {
        for y in [r.min.y - 10.0, r.max.y + 10.0] {
            candidates.push(vec![
                p,
                pos2(x1, p.y),
                pos2(x1, y),
                pos2(x2, y),
                pos2(x2, q.y),
                q,
            ]);
        }
    }
    candidates
}

fn obstacle_hits(path: &[Pos2], obstacles: &[Rect]) -> usize {
    path.windows(2)
        .map(|w| obstacles.iter().filter(|r| crosses(w[0], w[1], r)).count())
        .sum()
}

fn path_cost(path: &[Pos2]) -> f32 {
    let length: f32 = path.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    length + path.len() as f32 * 30.0
}

fn crosses(u: Pos2, v: Pos2, r: &Rect) -> bool {
    let (x0, x1) = (u.x.min(v.x), u.x.max(v.x));
    let (y0, y1) = (u.y.min(v.y), u.y.max(v.y));
    x0 < r.max.x && x1 > r.min.x && y0 < r.max.y && y1 > r.min.y
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
            ..Args::default()
        };
        assert_eq!(args(&["--stats", "shop.md"]), Ok(expected));
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
        ] {
            assert!(args(words).is_err(), "{words:?}");
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
    fn free_links_bend_halfway() {
        for step in 0..200 {
            let shift = step as f32 * 3.7;
            let p = pos2(150.3 + shift, 80.1);
            let q = pos2(420.9 + shift * 1.3, 260.7 + shift);
            let path = best_path(p, q, &[]);
            assert_eq!(path[1].x, (p.x + q.x) / 2.0, "from {p:?} to {q:?}");
        }
    }

    #[test]
    fn a_blocked_middle_moves_the_bend_next_to_the_start() {
        let (p, q) = (pos2(0.0, 0.0), pos2(300.0, 100.0));
        let middle = Rect::from_min_max(pos2(140.0, 30.0), pos2(160.0, 70.0));
        let path = best_path(p, q, &[middle]);
        assert_eq!(path[1].x, p.x + 22.0);
    }

    #[test]
    fn close_lines_spread_apart_wherever_they_are() {
        for step in 0..200 {
            let x = step as f32 * 0.25;
            let mut routes = vec![bent_at(x), bent_at(x + 3.0), bent_at(x + 100.0)];
            spread_side_by_side(&mut routes);
            assert_eq!(bends(&routes), [x - 4.5, x + 7.5, x + 100.0], "at {x}");
        }
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
        let edges: Vec<(usize, usize)> = ends.iter().flatten().copied().collect();
        let world = arrange::arrange(&sizes, &edges)
            .into_iter()
            .zip(&sizes)
            .map(|(p, s)| Rect::from_min_size(p, *s))
            .collect();
        (ends, world)
    }

    fn bent_at(x: f32) -> Option<Route> {
        Some(Route {
            pts: vec![
                pos2(-50.0, 0.0),
                pos2(x, 0.0),
                pos2(x, 100.0),
                pos2(250.0, 100.0),
            ],
            from_u: Side::Right.out(),
            to_u: Side::Left.out(),
        })
    }

    fn bends(routes: &[Option<Route>]) -> Vec<f32> {
        routes.iter().flatten().map(|r| r.pts[1].x).collect()
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
