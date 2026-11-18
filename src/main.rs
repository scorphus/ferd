#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod arrange;
mod layout;
#[cfg(target_os = "macos")]
mod macos;
mod mermaid;
mod model;
mod share;
mod sql;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
    pos2, vec2,
};
use notify::{RecursiveMode, Watcher};

use layout::{DiagramLayout, LayoutFile};
use model::{Card, Diagram, Link, Table};
use share::{Event, Msg, Session};

const FONT: f32 = 13.0;
const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 19.0;
const PAD: f32 = 10.0;
const TAG_W: f32 = 24.0;
const GAP: f32 = 70.0;

fn main() -> eframe::Result {
    #[cfg(target_os = "macos")]
    macos::listen();
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    let (path, ticket) = match first {
        Some(a) if a == "--join" => (None, args.next().map(|t| t.to_string_lossy().into_owned())),
        other => (other.map(PathBuf::from), None),
    };
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
            let mut app = App::default();
            if let Some(path) = path {
                app.open(&cc.egui_ctx, path);
            }
            if let Some(ticket) = ticket {
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
        self.session = Some(Session::host(ctx));
        self.status = "starting to share…".into();
    }

    fn join(&mut self, ctx: &egui::Context, ticket: &str) {
        match Session::join(ctx, ticket) {
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
        let visuals = ui.visuals().clone();
        painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
        let Some(src) = &mut self.source else { return };

        let measure = |text: &str| {
            painter
                .layout_no_wrap(text.to_string(), FontId::proportional(FONT), Color32::WHITE)
                .size()
                .x
        };
        let sizes: Vec<Vec2> = src
            .diagram()
            .tables
            .iter()
            .map(|t| box_size(t, &measure))
            .collect();
        place_new_tables(src, &sizes);

        let tables = src.diagram().tables.clone();
        if std::mem::take(&mut self.arrange_pending) {
            let index = |name: &str| tables.iter().position(|t| t.name == name);
            let edges: Vec<(usize, usize)> = src
                .links()
                .filter_map(|(l, _)| Some((index(&l.from)?, index(&l.to)?)))
                .collect();
            let placed = arrange::arrange(&sizes, &edges);
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
        let world: Vec<Rect> = tables
            .iter()
            .zip(&sizes)
            .map(|(t, s)| Rect::from_min_size(Pos2::from(dl.positions[&t.name]), *s))
            .collect();

        if self.fit_pending || matches!(self.export, Some(Export::Fitting { .. })) {
            self.view = fit(&world, rect);
            self.fit_pending = false;
        }

        let view = self.view;
        let to_world = |p: Pos2| ((p - rect.min - view.offset) / view.zoom).to_pos2();

        if let Some(pointer) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.0015).exp();
            if factor != 1.0 {
                let anchor = to_world(pointer);
                self.view.zoom = (self.view.zoom * factor).clamp(0.1, 4.0);
                self.view.offset = pointer - rect.min - anchor.to_vec2() * self.view.zoom;
            }
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let w = to_world(p);
            let hit = world
                .iter()
                .rposition(|r| r.contains(w))
                .map(|i| tables[i].name.clone());
            self.focus = if hit == self.focus { None } else { hit };
        }
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            let w = to_world(p);
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

        let view = self.view;
        let dl = src
            .layout
            .diagrams
            .get(src.title())
            .cloned()
            .unwrap_or_default();
        let screen: Vec<Rect> = tables
            .iter()
            .zip(&sizes)
            .map(|(t, s)| {
                let min = rect.min + view.offset + Vec2::from(dl.positions[&t.name]) * view.zoom;
                Rect::from_min_size(min, *s * view.zoom)
            })
            .collect();
        let index = |name: &str| tables.iter().position(|t| t.name == name);

        let links: Vec<(Link, bool)> = src.links().map(|(l, h)| (l.clone(), h)).collect();
        let ends: Vec<Option<(usize, usize)>> = links
            .iter()
            .map(|(l, _)| Some((index(&l.from)?, index(&l.to)?)))
            .collect();
        let routes = route_links(&ends, &screen, view.zoom);
        let focus = self.focus.as_ref().filter(|f| index(f).is_some());
        let lit = |e: (usize, usize)| match focus {
            Some(f) => tables[e.0].name == *f || tables[e.1].name == *f,
            None => true,
        };
        let base = visuals.text_color();
        let mut labels: Vec<(&Link, &Route)> = Vec::new();
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
                draw_route(&painter, link, *by_hand, route, view.zoom, color, &visuals);
                if pass {
                    labels.push((link, route));
                }
            }
        }
        draw_labels(&painter, &labels, &screen, view.zoom, &visuals);

        let mut order: Vec<usize> = (0..tables.len()).collect();
        if let Some(d) = &self.dragging
            && let Some(i) = index(d)
        {
            order.retain(|&j| j != i);
            order.push(i);
        }
        let hover = response.hover_pos().filter(|_| self.export.is_none());
        let mut tip: Option<String> = None;
        for i in order {
            if let Some(t) = draw_table(&painter, &tables[i], screen[i], view.zoom, &visuals, hover)
            {
                tip = Some(t);
            }
        }
        if let Some(tip) = tip
            && self.dragging.is_none()
        {
            response.clone().on_hover_text_at_pointer(tip);
        }

        let pointer = response
            .hover_pos()
            .map(|p| ((p - rect.min - view.offset) / view.zoom).into());
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
                    let at = rect.min + view.offset + Vec2::from(*pos) * view.zoom;
                    draw_peer(&painter, who, at);
                }
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
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

fn fit(world: &[Rect], screen: Rect) -> View {
    let Some(bounds) = world.iter().copied().reduce(|a, b| a.union(b)) else {
        return View::default();
    };
    let bounds = bounds.expand(30.0);
    let zoom = (screen.width() / bounds.width())
        .min(screen.height() / bounds.height())
        .clamp(0.1, 1.5);
    let offset = (screen.size() - bounds.size() * zoom) / 2.0 - bounds.min.to_vec2() * zoom;
    View { offset, zoom }
}

fn draw_table(
    painter: &egui::Painter,
    t: &Table,
    r: Rect,
    zoom: f32,
    visuals: &egui::Visuals,
    hover: Option<Pos2>,
) -> Option<String> {
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let border = visuals
        .widgets
        .noninteractive
        .fg_stroke
        .color
        .gamma_multiply(0.6);
    let radius = 5.0 * zoom;
    painter.rect_filled(r, radius, visuals.panel_fill);
    let header = Rect::from_min_size(r.min, vec2(r.width(), HEADER_H * zoom));
    painter.rect_filled(
        header,
        egui::CornerRadius {
            nw: radius as u8,
            ne: radius as u8,
            sw: 0,
            se: 0,
        },
        visuals.selection.bg_fill.gamma_multiply(0.45),
    );
    painter.rect_stroke(r, radius, Stroke::new(1.0, border), StrokeKind::Inside);

    let font = FONT * zoom;
    let show_text = font >= 4.0;
    if show_text {
        painter.text(
            pos2(r.min.x + PAD * zoom, header.center().y),
            Align2::LEFT_CENTER,
            &t.name,
            FontId::proportional(font * 1.1),
            text,
        );
    }
    let mut tip = None;
    if let Some(p) = hover
        && header.contains(p)
    {
        tip = t.note.clone();
    }
    for (i, c) in t.columns.iter().enumerate() {
        let top = r.min.y + (HEADER_H + i as f32 * ROW_H) * zoom;
        let row = Rect::from_min_max(pos2(r.min.x, top), pos2(r.max.x, top + ROW_H * zoom));
        let y = row.center().y + zoom;
        if show_text {
            let tag = match (c.pk, c.fk) {
                (true, true) => "PF",
                (true, false) => "PK",
                (false, true) => "FK",
                _ => "",
            };
            if !tag.is_empty() {
                let color = if c.pk {
                    Color32::from_rgb(214, 160, 40)
                } else {
                    Color32::from_rgb(90, 150, 220)
                };
                painter.text(
                    pos2(r.min.x + PAD * zoom, y),
                    Align2::LEFT_CENTER,
                    tag,
                    FontId::monospace(font * 0.8),
                    color,
                );
            }
            let name = if c.note.is_some() {
                format!("{} ·", c.name)
            } else {
                c.name.clone()
            };
            painter.text(
                pos2(r.min.x + (PAD + TAG_W) * zoom, y),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(font),
                text,
            );
            painter.text(
                pos2(r.max.x - PAD * zoom, y),
                Align2::RIGHT_CENTER,
                &c.ty,
                FontId::proportional(font),
                weak,
            );
        }
        if let Some(p) = hover
            && row.contains(p)
        {
            let mut lines = vec![format!("{} {}", c.name, c.ty)];
            lines.extend(c.note.clone());
            tip = Some(lines.join("\n"));
        }
    }
    tip
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

fn sides(a: Rect, b: Rect, z: f32) -> (Side, Side) {
    let room = 40.0 * z;
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

fn route_links(ends: &[Option<(usize, usize)>], boxes: &[Rect], z: f32) -> Vec<Option<Route>> {
    let mut slots: Slots = Default::default();
    let mut chosen = vec![None; ends.len()];
    for (i, end) in ends.iter().enumerate() {
        let Some((a, b)) = *end else { continue };
        let (sa, sb) = if a == b {
            (Side::Right, Side::Right)
        } else {
            sides(boxes[a], boxes[b], z)
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
                    let top = (r.min.y + HEADER_H * z).min(r.max.y);
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
    let mut middles: Vec<(usize, bool, f32)> = Vec::new();
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
            .map(|(_, r)| r.expand(4.0 * z))
            .collect();
        let pts = match (sa, sb) {
            (Side::Right, Side::Right) => {
                let x = boxes[a].max.x.max(boxes[b].max.x) + 30.0 * z;
                vec![p, pos2(x, p.y), pos2(x, q.y), q]
            }
            (s, _) if s.vertical() => best_path(p, q, &obstacles, z),
            _ => {
                let t = |p: Pos2| pos2(p.y, p.x);
                let flipped: Vec<Rect> = obstacles
                    .iter()
                    .map(|r| Rect::from_min_max(t(r.min), t(r.max)))
                    .collect();
                best_path(t(p), t(q), &flipped, z)
                    .into_iter()
                    .map(t)
                    .collect()
            }
        };
        if pts.len() == 4 {
            let vertical = pts[1].x == pts[2].x;
            middles.push((i, vertical, if vertical { pts[1].x } else { pts[1].y }));
        }
        routes.push(Some(Route {
            pts,
            from_u: sa.out(),
            to_u: sb.out(),
        }));
    }
    let mut buckets: std::collections::HashMap<(bool, i64), Vec<usize>> = Default::default();
    for &(i, vertical, c) in &middles {
        buckets
            .entry((vertical, (c / (40.0 * z)).round() as i64))
            .or_default()
            .push(i);
    }
    for (&(vertical, _), list) in &buckets {
        let n = list.len() as f32;
        for (k, &i) in list.iter().enumerate() {
            let shift = (k as f32 - (n - 1.0) / 2.0) * 9.0 * z;
            if let Some(route) = &mut routes[i] {
                for p in &mut route.pts[1..3] {
                    if vertical {
                        p.x += shift;
                    } else {
                        p.y += shift;
                    }
                }
            }
        }
    }
    routes
}

fn best_path(p: Pos2, q: Pos2, obstacles: &[Rect], z: f32) -> Vec<Pos2> {
    let d = if q.x >= p.x { 1.0 } else { -1.0 };
    let (x1, x2) = (p.x + d * 22.0 * z, q.x - d * 22.0 * z);
    let mut candidates: Vec<Vec<Pos2>> = [(p.x + q.x) / 2.0, x1, x2]
        .into_iter()
        .map(|x| vec![p, pos2(x, p.y), pos2(x, q.y), q])
        .collect();
    let (lo, hi) = (p.x.min(q.x), p.x.max(q.x));
    for r in obstacles.iter().filter(|r| r.max.x > lo && r.min.x < hi) {
        for y in [r.min.y - 10.0 * z, r.max.y + 10.0 * z] {
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
    let cost = |path: &Vec<Pos2>| {
        let hits = path
            .windows(2)
            .map(|w| obstacles.iter().filter(|r| crosses(w[0], w[1], r)).count())
            .sum::<usize>();
        let length: f32 = path.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        hits as f32 * 1e6 + length + path.len() as f32 * 30.0 * z
    };
    candidates
        .into_iter()
        .min_by(|a, b| cost(a).total_cmp(&cost(b)))
        .unwrap()
}

fn crosses(u: Pos2, v: Pos2, r: &Rect) -> bool {
    let (x0, x1) = (u.x.min(v.x), u.x.max(v.x));
    let (y0, y1) = (u.y.min(v.y), u.y.max(v.y));
    x0 < r.max.x && x1 > r.min.x && y0 < r.max.y && y1 > r.min.y
}

#[allow(clippy::too_many_arguments)]
fn draw_route(
    painter: &egui::Painter,
    link: &Link,
    by_hand: bool,
    route: &Route,
    zoom: f32,
    color: Color32,
    visuals: &egui::Visuals,
) {
    let stroke = Stroke::new(1.4, color);
    if by_hand {
        painter.extend(Shape::dashed_line(
            &route.pts,
            stroke,
            7.0 * zoom,
            5.0 * zoom,
        ));
    } else {
        painter.add(Shape::line(route.pts.clone(), stroke));
    }
    let last = route.pts.len() - 1;
    mark(
        painter,
        route.pts[0],
        route.from_u,
        link.from_card,
        zoom,
        stroke,
        visuals,
    );
    mark(
        painter,
        route.pts[last],
        route.to_u,
        link.to_card,
        zoom,
        stroke,
        visuals,
    );
}

fn mark(
    painter: &egui::Painter,
    p: Pos2,
    u: Vec2,
    card: Card,
    zoom: f32,
    stroke: Stroke,
    visuals: &egui::Visuals,
) {
    let n = vec2(-u.y, u.x);
    let s = zoom;
    let bar = |d: f32| {
        painter.line_segment(
            [p + u * d * s + n * 6.0 * s, p + u * d * s - n * 6.0 * s],
            stroke,
        )
    };
    let ring = |d: f32| {
        painter.circle_filled(p + u * d * s, 4.0 * s, visuals.extreme_bg_color);
        painter.circle_stroke(p + u * d * s, 4.0 * s, stroke);
    };
    let foot = || {
        let tip = p + u * 11.0 * s;
        painter.line_segment([tip, p + n * 7.0 * s], stroke);
        painter.line_segment([tip, p - n * 7.0 * s], stroke);
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

fn draw_labels(
    painter: &egui::Painter,
    labels: &[(&Link, &Route)],
    boxes: &[Rect],
    zoom: f32,
    visuals: &egui::Visuals,
) {
    let font = FONT * 0.85 * zoom;
    if font < 4.0 {
        return;
    }
    let mut taken: Vec<Rect> = Vec::new();
    for (link, route) in labels {
        if link.label.is_empty() {
            continue;
        }
        let galley = painter.layout_no_wrap(
            link.label.clone(),
            FontId::proportional(font),
            visuals.text_color(),
        );
        let size = galley.size() + Vec2::splat(4.0 * zoom);
        let r = label_spot(&route.pts, size, boxes, &taken);
        taken.push(r);
        painter.rect_filled(r, 3.0 * zoom, visuals.extreme_bg_color.gamma_multiply(0.9));
        painter.galley(
            r.min + Vec2::splat(2.0 * zoom),
            galley,
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
}
