use std::sync::mpsc;

use eframe::egui;
use iroh::endpoint::{RecvStream, SendStream, presets};
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
}

impl Session {
    pub fn host(ctx: &egui::Context) -> Session {
        Session::start(ctx, None)
    }

    pub fn join(ctx: &egui::Context, ticket: &str) -> Result<Session, String> {
        let addr = decode_ticket(ticket)?;
        Ok(Session::start(ctx, Some(addr)))
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.out.send(msg);
    }

    fn start(ctx: &egui::Context, join: Option<EndpointAddr>) -> Session {
        let (out, rx) = unbounded_channel();
        let (tx, events) = mpsc::channel();
        let notify = Notify {
            tx,
            ctx: ctx.clone(),
        };
        let hosting = join.is_none();
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
                    None => run_host(rx, &notify).await,
                    Some(addr) => run_guest(addr, rx, &notify).await,
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
        }
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

async fn run_host(mut rx: UnboundedReceiver<Msg>, notify: &Notify) -> Result<(), String> {
    let ep = endpoint(true).await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), ep.online()).await;
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
                tokio::spawn(async move {
                    let result = async {
                        let conn = incoming.await.map_err(|e| e.to_string())?;
                        let (send, recv) = conn.accept_bi().await.map_err(|e| e.to_string())?;
                        pump(send, recv, peer_rx, |m| { let _ = from_peers.send((id, m)); }).await
                    }.await;
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
) -> Result<(), String> {
    let ep = endpoint(false).await?;
    notify.status("connecting…".into());
    let conn = ep.connect(addr, ALPN).await.map_err(|e| e.to_string())?;
    let (mut send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    write(&mut send, &Msg::Hello { who: who() }).await?;
    notify.status("connected".into());
    let result = pump(send, recv, rx, |m| notify.send(Event::Msg(m))).await;
    ep.close().await;
    result.and(Err("the host ended the session".into()))
}

async fn pump(
    mut send: SendStream,
    mut recv: RecvStream,
    mut out: UnboundedReceiver<Msg>,
    mut on_msg: impl FnMut(Msg),
) -> Result<(), String> {
    let writer = async {
        while let Some(msg) = out.recv().await {
            write(&mut send, &msg).await?;
        }
        Ok::<_, String>(())
    };
    let reader = async {
        loop {
            on_msg(read(&mut recv).await?);
        }
    };
    tokio::select! {
        r = writer => r,
        r = reader => r,
    }
}

async fn write(send: &mut SendStream, msg: &Msg) -> Result<(), String> {
    let body = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    send.write_all(&(body.len() as u32).to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    send.write_all(&body).await.map_err(|e| e.to_string())
}

async fn read(recv: &mut RecvStream) -> Result<Msg, String> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(format!("message of {len} bytes is too large"));
    }
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
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
        let host = Session::host(&ctx);
        let ticket = loop {
            match host
                .events
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap()
            {
                Event::Ticket(t) => break t,
                _ => continue,
            }
        };
        let guest = Session::join(&ctx, &ticket).unwrap();
        let joined = loop {
            match host
                .events
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap()
            {
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
            match guest
                .events
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap()
            {
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
            match host
                .events
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap()
            {
                Event::Msg(m) => break m,
                _ => continue,
            }
        };
        assert!(matches!(got, Msg::Move { pos, .. } if pos == [1.0, 2.0]));
    }
}
