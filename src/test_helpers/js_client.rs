use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::engine::js_client::{JsClient, JsExecutorClient};

/// A global, lazily-initialized, thread-safe cell for our client.
static JS_CLIENT: OnceCell<Arc<dyn JsClient>> = OnceCell::const_new();

/// Gets a shared instance of the JsExecutorClient.
/// The first test to call this will block while compiling and starting the
/// js_executor. All subsequent calls (even from parallel tests) will get the
/// same instance instantly.
pub async fn get_shared_js_client() -> Arc<dyn JsClient> {
    JS_CLIENT
        .get_or_init(|| async {
            let client = JsExecutorClient::new()
                .await
                .expect("FATAL: Failed to start shared JsExecutorClient for tests");

            let client_arc: Arc<dyn JsClient> = Arc::new(client);
            client_arc
        })
        .await
        .clone()
}
