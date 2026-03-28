use mcp_server::Context;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

const EXTENSION_KEY: &str = "progress";

pub struct ProgressSender {
    tx: mpsc::Sender<String>,
}

impl ProgressSender {
    pub fn new(tx: mpsc::Sender<String>) -> Self {
        ProgressSender { tx }
    }

    pub async fn send_progress(
        &self,
        token: &str,
        progress: u64,
        total: Option<u64>,
        message: &str,
    ) {
        let mut params = json!({
            "progressToken": token,
            "progress": progress,
            "message": message,
        });

        if let Some(t) = total {
            params["total"] = json!(t);
        }

        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": params,
        });

        let msg = serde_json::to_string(&notification).unwrap_or_default();
        let _ = self.tx.send(msg).await;
    }
}

pub fn extract_progress_sender(ctx: &Context) -> Option<Arc<ProgressSender>> {
    ctx.get_extension::<ProgressSender>(EXTENSION_KEY)
}

pub fn store_progress_sender(ctx: &mut Context, sender: ProgressSender) {
    ctx.set_extension(EXTENSION_KEY, sender);
}
