//! aria2 JSON-RPC over a single WebSocket.
//!
//! Responses are matched to calls by id. aria2's push notifications
//! (`aria2.onDownloadStart`, ...) arrive on a separate channel, and `closed`
//! fires when the socket goes away so the supervisor can restart the engine.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// aria2-next answers `aria2.probe` within 20 s; nothing else should take longer.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

type Reply = oneshot::Sender<Result<Value, String>>;

#[derive(Clone)]
pub struct Rpc {
    inner: Arc<Inner>,
}

struct Inner {
    token: String,
    next_id: AtomicU64,
    outgoing: mpsc::UnboundedSender<(u64, String, Reply)>,
}

/// A push notification from aria2. Every aria2 notification names one task.
pub struct Notification {
    pub gid: String,
}

pub struct Connection {
    pub rpc: Rpc,
    pub notifications: mpsc::UnboundedReceiver<Notification>,
    pub closed: oneshot::Receiver<()>,
}

impl Rpc {
    pub async fn connect(port: u16, secret: &str) -> Result<Connection, String> {
        let url = format!("ws://127.0.0.1:{port}/jsonrpc");
        let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .map_err(|error| error.to_string())?;
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let (notify_tx, notifications) = mpsc::unbounded_channel();
        let (closed_tx, closed) = oneshot::channel();
        tokio::spawn(async move {
            run_socket(socket, outgoing_rx, notify_tx).await;
            let _ = closed_tx.send(());
        });
        let rpc = Rpc {
            inner: Arc::new(Inner {
                token: format!("token:{secret}"),
                next_id: AtomicU64::new(1),
                outgoing,
            }),
        };
        Ok(Connection {
            rpc,
            notifications,
            closed,
        })
    }

    /// Calls one aria2 method. The secret token is added in front of `params`.
    pub async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, String> {
        self.send(method, self.with_token(params)).await
    }

    /// Runs several calls in one round trip; each call gets its own result.
    pub async fn multicall(
        &self,
        calls: Vec<(&str, Vec<Value>)>,
    ) -> Result<Vec<Result<Value, String>>, String> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<Value> = calls
            .into_iter()
            .map(|(method, params)| json!({ "methodName": method, "params": self.with_token(params) }))
            .collect();
        // system.multicall itself takes no token; each nested call carries one.
        let answer = self
            .send("system.multicall", vec![Value::Array(list)])
            .await?;
        let Value::Array(items) = answer else {
            return Err("system.multicall returned a non-array".into());
        };
        Ok(items
            .into_iter()
            .map(|item| match item {
                // Success is wrapped in a one-element array; failure is a fault struct.
                Value::Array(mut values) if values.len() == 1 => Ok(values.remove(0)),
                fault => Err(error_message(&fault)),
            })
            .collect())
    }

    fn with_token(&self, params: Vec<Value>) -> Vec<Value> {
        let mut full = Vec::with_capacity(params.len() + 1);
        full.push(Value::String(self.inner.token.clone()));
        full.extend(params);
        full
    }

    async fn send(&self, method: &str, params: Vec<Value>) -> Result<Value, String> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let text =
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        let (reply, answer) = oneshot::channel();
        self.inner
            .outgoing
            .send((id, text, reply))
            .map_err(|_| "engine connection closed".to_string())?;
        match tokio::time::timeout(CALL_TIMEOUT, answer).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("engine connection closed".into()),
            Err(_) => Err(format!("{method} timed out")),
        }
    }
}

async fn run_socket<S>(
    socket: S,
    mut outgoing: mpsc::UnboundedReceiver<(u64, String, Reply)>,
    notifications: mpsc::UnboundedSender<Notification>,
) where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures_util::Sink<Message>
        + Unpin,
{
    let (mut sink, mut stream) = socket.split();
    let mut pending: HashMap<u64, Reply> = HashMap::new();
    loop {
        tokio::select! {
            next = outgoing.recv() => {
                let Some((id, text, reply)) = next else { break };
                pending.insert(id, reply);
                if sink.send(Message::text(text)).await.is_err() {
                    break;
                }
            }
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => dispatch(text.as_str(), &mut pending, &notifications),
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
    // Dropping `pending` fails every call still waiting for an answer.
}

fn dispatch(
    text: &str,
    pending: &mut HashMap<u64, Reply>,
    notifications: &mpsc::UnboundedSender<Notification>,
) {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        if let Some(reply) = pending.remove(&id) {
            let result = match message.get("error") {
                Some(error) => Err(error_message(error)),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = reply.send(result);
        }
    } else if let Some(gid) = message.pointer("/params/0/gid").and_then(Value::as_str) {
        let _ = notifications.send(Notification {
            gid: gid.to_string(),
        });
    }
}

fn error_message(error: &Value) -> String {
    error
        .get("message")
        .or_else(|| error.get("faultString"))
        .and_then(Value::as_str)
        .unwrap_or("engine error")
        .to_string()
}
