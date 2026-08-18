use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::state::{ServerMessage, SharedMarkdownState};

pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedMarkdownState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

enum WsAction {
    Forward(ServerMessage),
    Stop,
}

/// A client that stalls (laptop asleep, slow socket) falls behind the broadcast
/// channel and gets `Lagged`. Dropping the socket there left the page frozen on
/// whatever it had rendered, so collapse the missed burst into one reload.
fn next_action(recv: Result<ServerMessage, RecvError>) -> WsAction {
    match recv {
        Ok(msg) => WsAction::Forward(msg),
        Err(RecvError::Lagged(_)) => WsAction::Forward(ServerMessage::Reload),
        Err(RecvError::Closed) => WsAction::Stop,
    }
}

async fn handle_websocket(socket: WebSocket, state: SharedMarkdownState) {
    let (mut sender, mut receiver) = socket.split();

    let (mut change_rx, initial_msg) = {
        let guard = state.lock().await;
        let rx = guard.change_tx.subscribe();
        let msg = if guard.daemon_mode && guard.has_workspace() {
            Some(crate::state::ServerMessage::WorkspaceChanged {
                base_dir: guard.base_dir.display().to_string(),
                file: None,
            })
        } else {
            None
        };
        (rx, msg)
    };

    // if a workspace is already loaded, notify this client immediately
    if let Some(ref msg) = initial_msg {
        eprintln!("[ws] client connected, workspace loaded -- sending WorkspaceChanged");
        if let Ok(json) = serde_json::to_string(msg) {
            let _ = sender.send(Message::Text(json)).await;
        }
    } else {
        eprintln!("[ws] client connected, no workspace loaded");
    }

    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(_)) => {}
                Ok(Message::Close(_)) => break,
                _ => {}
            }
        }
    });

    let mut send_task = tokio::spawn(async move {
        while let WsAction::Forward(msg) = next_action(change_rx.recv().await) {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // whichever half finishes first, abort the other -- dropping its handle only
    // detaches it, leaving the socket and its broadcast subscription alive
    tokio::select! {
        _ = &mut recv_task => send_task.abort(),
        _ = &mut send_task => recv_task.abort(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lagged_client_gets_a_reload_not_a_dropped_socket() {
        assert!(matches!(
            next_action(Err(RecvError::Lagged(42))),
            WsAction::Forward(ServerMessage::Reload)
        ));
    }

    #[test]
    fn closed_channel_stops_the_send_loop() {
        assert!(matches!(
            next_action(Err(RecvError::Closed)),
            WsAction::Stop
        ));
    }

    #[test]
    fn normal_message_is_forwarded_unchanged() {
        let msg = ServerMessage::FileMoved {
            from: "a.md".into(),
            to: "b.md".into(),
        };
        assert!(matches!(
            next_action(Ok(msg)),
            WsAction::Forward(ServerMessage::FileMoved { .. })
        ));
    }
}
