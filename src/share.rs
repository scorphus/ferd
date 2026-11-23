use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use eframe::egui;
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::layout::LayoutFile;
use crate::model::Link;

const ALPN: &[u8] = b"ferd/share/0";
const MAX_MESSAGE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    Hello {
        who: String,
    },
    Source {
        name: String,
        text: String,
    },
    Layout(LayoutFile),
    Move {
        diagram: String,
        table: String,
        pos: [f32; 2],
    },
    Links {
        diagram: String,
        links: Vec<Link>,
    },
    Pointer {
        who: String,
        diagram: String,
        pos: Option<[f32; 2]>,
    },
}

impl Msg {
    fn kind(&self) -> &'static str {
        match self {
            Msg::Hello { .. } => "hello",
            Msg::Source { .. } => "source",
            Msg::Layout(_) => "layout",
            Msg::Move { .. } => "move",
            Msg::Links { .. } => "links",
            Msg::Pointer { .. } => "pointer",
        }
    }
}

pub enum Event {
    Ticket(String),
    Status(String),
    Ended(String),
    Joined(String),
    Msg(Msg),
}

pub struct Session {
    pub hosting: bool,
    out: UnboundedSender<Msg>,
    pub events: mpsc::Receiver<Event>,
    meter: Meter,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Tally {
    pub messages: u64,
    pub bytes: u64,
}

impl Tally {
    fn add(&mut self, bytes: usize) {
        self.messages += 1;
        self.bytes += bytes as u64;
    }

    pub fn total<'a>(tallies: impl IntoIterator<Item = &'a Tally>) -> Tally {
        tallies.into_iter().fold(Tally::default(), |sum, t| Tally {
            messages: sum.messages + t.messages,
            bytes: sum.bytes + t.bytes,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Wire {
    pub direct: bool,
    pub rtt: Option<Duration>,
    pub sent: u64,
    pub received: u64,
    pub lost_packets: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Flow {
    pub messages_out: f64,
    pub messages_in: f64,
    pub wire_out: f64,
    pub wire_in: f64,
}

#[derive(Debug, Default, Clone)]
pub struct Traffic {
    pub sent: BTreeMap<&'static str, Tally>,
    pub received: BTreeMap<&'static str, Tally>,
    pub wires: Vec<Wire>,
    pub per_second: Flow,
}

#[derive(Default)]
struct Counters {
    sent: BTreeMap<&'static str, Tally>,
    received: BTreeMap<&'static str, Tally>,
    connections: Vec<(usize, Connection)>,
    history: VecDeque<(Instant, Flow)>,
}

type Meter = Option<Arc<Mutex<Counters>>>;

fn count(meter: &Meter, f: impl FnOnce(&mut Counters)) {
    if let Some(counters) = meter {
        f(&mut counters.lock().unwrap());
    }
}

impl Session {
    pub fn host(ctx: &egui::Context, measure: bool) -> Session {
        Session::start(ctx, None, measure)
    }

    pub fn join(ctx: &egui::Context, ticket: &str, measure: bool) -> Result<Session, String> {
        let addr = decode_ticket(ticket)?;
        Ok(Session::start(ctx, Some(addr), measure))
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.out.send(msg);
    }

    pub fn traffic(&self) -> Option<Traffic> {
        let mut counters = self.meter.as_ref()?.lock().unwrap();
        let wires: Vec<Wire> = counters.connections.iter().map(|(_, c)| wire(c)).collect();
        let totals = Flow {
            messages_out: Tally::total(counters.sent.values()).messages as f64,
            messages_in: Tally::total(counters.received.values()).messages as f64,
            wire_out: wires.iter().map(|w| w.sent as f64).sum(),
            wire_in: wires.iter().map(|w| w.received as f64).sum(),
        };
        let per_second = per_second(&mut counters.history, Instant::now(), totals);
        Some(Traffic {
            sent: counters.sent.clone(),
            received: counters.received.clone(),
            wires,
            per_second,
        })
    }

    fn start(ctx: &egui::Context, join: Option<EndpointAddr>, measure: bool) -> Session {
        let (out, rx) = unbounded_channel();
        let (tx, events) = mpsc::channel();
        let notify = Notify {
            tx,
            ctx: ctx.clone(),
        };
        let hosting = join.is_none();
        let meter: Meter = measure.then(Default::default);
        let session_meter = meter.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => return notify.send(Event::Ended(format!("sharing failed: {e}"))),
            };
            let result = rt.block_on(async {
                match join {
                    None => run_host(rx, &notify, &session_meter).await,
                    Some(addr) => run_guest(addr, rx, &notify, &session_meter).await,
                }
            });
            notify.send(Event::Ended(match result {
                Ok(()) => "sharing ended".into(),
                Err(e) => format!("sharing ended: {e}"),
            }));
        });
        Session {
            hosting,
            out,
            events,
            meter,
        }
    }
}

fn wire(conn: &Connection) -> Wire {
    let stats = conn.stats();
    let paths = conn.paths();
    let selected = paths.iter().find(|path| path.is_selected());
    Wire {
        direct: selected.as_ref().is_some_and(|path| path.is_ip()),
        rtt: selected.map(|path| path.rtt()),
        sent: stats.udp_tx.bytes,
        received: stats.udp_rx.bytes,
        lost_packets: stats.lost_packets,
    }
}

fn per_second(history: &mut VecDeque<(Instant, Flow)>, now: Instant, totals: Flow) -> Flow {
    history.push_back((now, totals));
    while history
        .front()
        .is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_secs(1))
    {
        history.pop_front();
    }
    let (start, first) = history[0];
    let seconds = now.duration_since(start).as_secs_f64();
    if seconds <= 0.0 {
        return Flow::default();
    }
    let rate = |later: f64, earlier: f64| ((later - earlier) / seconds).max(0.0);
    Flow {
        messages_out: rate(totals.messages_out, first.messages_out),
        messages_in: rate(totals.messages_in, first.messages_in),
        wire_out: rate(totals.wire_out, first.wire_out),
        wire_in: rate(totals.wire_in, first.wire_in),
    }
}

pub fn who() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "peer".into())
}

#[derive(Clone)]
struct Notify {
    tx: mpsc::Sender<Event>,
    ctx: egui::Context,
}

impl Notify {
    fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ctx.request_repaint();
    }

    fn status(&self, s: String) {
        self.send(Event::Status(s));
    }
}

async fn endpoint(host: bool) -> Result<Endpoint, String> {
    let mut builder = Endpoint::builder(presets::N0);
    if let Ok(url) = std::env::var("FERD_RELAY") {
        let url: RelayUrl = url.parse().map_err(|e| format!("FERD_RELAY {url}: {e}"))?;
        builder = builder.relay_mode(RelayMode::custom([url]));
    }
    if host {
        builder = builder.alpns(vec![ALPN.to_vec()]);
    }
    builder.bind().await.map_err(|e| e.to_string())
}

async fn run_host(
    mut rx: UnboundedReceiver<Msg>,
    notify: &Notify,
    meter: &Meter,
) -> Result<(), String> {
    let ep = endpoint(true).await?;
    let _ = tokio::time::timeout(Duration::from_secs(10), ep.online()).await;
    notify.send(Event::Ticket(encode_ticket(&ep.addr())?));
    let (from_peers, mut peer_msgs) = unbounded_channel::<(usize, Msg)>();
    let mut peers: Vec<(usize, UnboundedSender<Msg>)> = Vec::new();
    let mut next_id = 0;
    loop {
        tokio::select! {
            incoming = ep.accept() => {
                let Some(incoming) = incoming else { break };
                let id = next_id;
                next_id += 1;
                let (to_peer, peer_rx) = unbounded_channel();
                peers.push((id, to_peer));
                let from_peers = from_peers.clone();
                let notify = notify.clone();
                let meter = meter.clone();
                tokio::spawn(async move {
                    let result = async {
                        let conn = incoming.await.map_err(|e| e.to_string())?;
                        count(&meter, |c| c.connections.push((id, conn.clone())));
                        let (send, recv) = conn.accept_bi().await.map_err(|e| e.to_string())?;
                        pump(send, recv, peer_rx, |m| { let _ = from_peers.send((id, m)); }, &meter).await
                    }.await;
                    count(&meter, |c| c.connections.retain(|(other, _)| *other != id));
                    notify.status(match result {
                        Ok(()) => "a peer left".into(),
                        Err(e) => format!("a peer left: {e}"),
                    });
                });
            }
            Some((id, msg)) = peer_msgs.recv() => {
                if let Msg::Hello { who } = &msg {
                    notify.send(Event::Joined(who.clone()));
                    continue;
                }
                peers.retain(|(other, tx)| *other == id || tx.send(msg.clone()).is_ok());
                notify.send(Event::Msg(msg));
            }
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                peers.retain(|(_, tx)| tx.send(msg.clone()).is_ok());
            }
        }
    }
    ep.close().await;
    Ok(())
}

async fn run_guest(
    addr: EndpointAddr,
    rx: UnboundedReceiver<Msg>,
    notify: &Notify,
    meter: &Meter,
) -> Result<(), String> {
    let ep = endpoint(false).await?;
    notify.status("connecting…".into());
    let conn = ep.connect(addr, ALPN).await.map_err(|e| e.to_string())?;
    count(meter, |c| c.connections.push((0, conn.clone())));
    let (mut send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write(&mut send, &Msg::Hello { who: who() }, meter).await?;
    notify.status("connected".into());
    let result = pump(send, recv, rx, |m| notify.send(Event::Msg(m)), meter).await;
    count(meter, |c| c.connections.clear());
    ep.close().await;
    result.and(Err("the host ended the session".into()))
}

async fn pump(
    mut send: SendStream,
    mut recv: RecvStream,
    mut out: UnboundedReceiver<Msg>,
    mut on_msg: impl FnMut(Msg),
    meter: &Meter,
) -> Result<(), String> {
    let writer = async {
        while let Some(msg) = out.recv().await {
            write(&mut send, &msg, meter).await?;
        }
        Ok::<_, String>(())
    };
    let reader = async {
        loop {
            on_msg(read(&mut recv, meter).await?);
        }
    };
    tokio::select! {
        r = writer => r,
        r = reader => r,
    }
}

async fn write(send: &mut SendStream, msg: &Msg, meter: &Meter) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    let len = (body.len() as u32).to_be_bytes();
    send.write_all(&len).await.map_err(|e| e.to_string())?;
    send.write_all(&body).await.map_err(|e| e.to_string())?;
    let bytes = len.len() + body.len();
    count(meter, |c| c.sent.entry(msg.kind()).or_default().add(bytes));
    Ok(())
}

async fn read(recv: &mut RecvStream, meter: &Meter) -> Result<Msg, String> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let body_len = u32::from_be_bytes(len) as usize;
    if body_len > MAX_MESSAGE {
        return Err(format!("message of {body_len} bytes is too large"));
    }
    let mut body = vec![0u8; body_len];
    recv.read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    let msg: Msg = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let bytes = len.len() + body_len;
    count(meter, |c| {
        c.received.entry(msg.kind()).or_default().add(bytes)
    });
    Ok(msg)
}

fn encode_ticket(addr: &EndpointAddr) -> Result<String, String> {
    let json = serde_json::to_vec(addr).map_err(|e| e.to_string())?;
    Ok(format!(
        "ferd{}",
        json.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

fn decode_ticket(ticket: &str) -> Result<EndpointAddr, String> {
    let hex = ticket
        .trim()
        .strip_prefix("ferd")
        .ok_or("not a ferd ticket")?;
    if hex.len() % 2 != 0 || !hex.is_ascii() {
        return Err("not a ferd ticket".into());
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| "not a ferd ticket")?;
    serde_json::from_slice(&bytes).map_err(|e| format!("not a ferd ticket: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_round_trip() {
        let key = iroh::SecretKey::generate();
        let addr = EndpointAddr::new(key.public());
        assert_eq!(decode_ticket(&encode_ticket(&addr).unwrap()).unwrap(), addr);
        assert!(decode_ticket("ferdzz").is_err());
    }

    #[test]
    fn host_and_guest_exchange_messages() {
        let ctx = egui::Context::default();
        let host = Session::host(&ctx, true);
        let ticket = loop {
            match host.events.recv_timeout(Duration::from_secs(30)).unwrap() {
                Event::Ticket(t) => break t,
                _ => continue,
            }
        };
        let guest = Session::join(&ctx, &ticket, true).unwrap();
        let joined = loop {
            match host.events.recv_timeout(Duration::from_secs(30)).unwrap() {
                Event::Joined(who) => break who,
                _ => continue,
            }
        };
        assert_eq!(joined, who());
        host.send(Msg::Source {
            name: "a.md".into(),
            text: "x".into(),
        });
        let got = loop {
            match guest.events.recv_timeout(Duration::from_secs(30)).unwrap() {
                Event::Msg(m) => break m,
                _ => continue,
            }
        };
        assert!(matches!(got, Msg::Source { name, .. } if name == "a.md"));
        guest.send(Msg::Move {
            diagram: "d".into(),
            table: "t".into(),
            pos: [1.0, 2.0],
        });
        let got = loop {
            match host.events.recv_timeout(Duration::from_secs(30)).unwrap() {
                Event::Msg(m) => break m,
                _ => continue,
            }
        };
        assert!(matches!(got, Msg::Move { pos, .. } if pos == [1.0, 2.0]));

        let messages = |tallies: &BTreeMap<&str, Tally>, kind: &str| tallies[kind].messages;
        let hosted = host.traffic().unwrap();
        assert_eq!(messages(&hosted.received, "hello"), 1);
        assert_eq!(messages(&hosted.received, "move"), 1);
        assert_eq!(messages(&hosted.sent, "source"), 1);
        assert_eq!(hosted.wires.len(), 1);
        let joined = guest.traffic().unwrap();
        assert_eq!(messages(&joined.sent, "hello"), 1);
        assert_eq!(messages(&joined.received, "source"), 1);
        assert!(joined.wires[0].rtt.is_some());
        assert!(joined.wires[0].sent > 0 && joined.wires[0].received > 0);
    }

    #[test]
    fn nothing_is_counted_without_a_meter() {
        let mut counted = false;
        count(&None, |_| counted = true);
        assert!(!counted);
        let (out, _) = unbounded_channel();
        let (_, events) = mpsc::channel();
        let session = Session {
            hosting: true,
            out,
            events,
            meter: None,
        };
        assert!(session.traffic().is_none());
    }

    #[test]
    fn flow_is_the_change_over_the_last_second() {
        let mut history = VecDeque::new();
        let start = Instant::now();
        let totals = |n: f64| Flow {
            messages_out: n,
            messages_in: 2.0 * n,
            wire_out: 100.0 * n,
            wire_in: 0.0,
        };
        for tenth in 0..=30 {
            let at = start + Duration::from_millis(tenth * 100);
            let flow = per_second(&mut history, at, totals(tenth as f64));
            if tenth >= 10 {
                assert_eq!(flow, totals(10.0), "at {tenth}");
            }
        }
    }
}
