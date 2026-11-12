//! # HTTP Server for Receipt and Pattern Printing
//!
//! Provides a web interface for printing text receipts and visual patterns via HTTP.
//!
//! ## Usage
//!
//! ```bash
//! estrella serve --listen 0.0.0.0:8080 --device /dev/rfcomm0
//! ```
//!
//! Then open http://localhost:8080 in a browser to access the UI.

mod handlers;
mod state;
mod static_files;

pub use state::{CachedIntensity, IntensityCacheKey, PhotoSession, ServerConfig};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::EstrellaError;
use state::{AppState, SESSION_EXPIRATION_SECS};

/// Start the HTTP server.
///
/// ## Example
///
/// ```no_run
/// use estrella::server::{serve, ServerConfig};
///
/// # async fn example() -> Result<(), estrella::error::EstrellaError> {
/// use estrella::printer::Protocol;
/// use estrella::transport::{Printer, PrinterTarget};
///
/// let config = ServerConfig {
///     printer: Printer {
///         protocol: Protocol::Star,
///         target: PrinterTarget::Device("/dev/rfcomm0".to_string()),
///     },
///     listen_addr: "0.0.0.0:8080".to_string(),
///     templates_dir: None,
/// };
///
/// serve(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn serve(config: ServerConfig) -> Result<(), EstrellaError> {
    let app_state = Arc::new(AppState::new(config.clone()));

    // Spawn background cache cleanup task
    tokio::spawn(cleanup_caches(app_state.clone()));

    let app = router(app_state);

    println!("Estrella HTTP server starting...");
    println!("Listening on: {}", config.listen_addr);
    println!(
        "Printer: {} ({:?})",
        config.printer.target, config.printer.protocol
    );
    if let Some(dir) = &config.templates_dir {
        println!("Templates: {}", dir.display());
    }
    println!();
    println!(
        "Open http://{}/ in your browser to print",
        config.listen_addr
    );
    println!();

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .map_err(|e| {
            EstrellaError::Transport(format!("Failed to bind to {}: {}", config.listen_addr, e))
        })?;

    axum::serve(listener, app)
        .await
        .map_err(|e| EstrellaError::Transport(format!("Server error: {}", e)))?;

    Ok(())
}

fn router(app_state: Arc<AppState>) -> Router {
    Router::new()
        // Liveness/readiness. Deliberately doesn't touch the printer: a printer
        // that's switched off shouldn't take the web UI down with it.
        .route("/healthz", get(|| async { "ok" }))
        // Frontend
        .route("/", get(static_files::index_handler))
        .route("/assets/{*path}", get(static_files::asset_handler))
        // JSON API
        .route("/api/json/preview", post(handlers::json_api::preview))
        .route("/api/json/print", post(handlers::json_api::print))
        .route(
            "/api/json/canvas-layout",
            post(handlers::json_api::canvas_layout),
        )
        .route(
            "/api/json/component/{type}/default",
            get(handlers::json_api::component_default),
        )
        // Receipt API
        .route("/api/receipt/print", post(handlers::receipt::print))
        .route("/api/receipt/preview", post(handlers::receipt::preview))
        // Pattern API
        .route("/api/patterns", get(handlers::patterns::list))
        .route(
            "/api/patterns/{name}/params",
            get(handlers::patterns::params),
        )
        .route(
            "/api/patterns/{name}/preview",
            get(handlers::patterns::preview),
        )
        .route(
            "/api/patterns/{name}/randomize",
            post(handlers::patterns::randomize),
        )
        .route(
            "/api/patterns/{name}/print",
            post(handlers::patterns::print),
        )
        // Weave API
        .route("/api/weave/preview", post(handlers::weave::preview))
        .route("/api/weave/print", post(handlers::weave::print))
        // Photo API (50MB limit for uploads)
        .route(
            "/api/photo/upload",
            post(handlers::photo::upload).layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route("/api/photo/{id}/preview", get(handlers::photo::preview))
        .route("/api/photo/{id}/print", post(handlers::photo::print))
        // Templates
        .route("/api/templates", get(handlers::templates::list))
        .route(
            "/api/templates/{id}/preview",
            post(handlers::templates::preview),
        )
        .route(
            "/api/templates/{id}/print",
            post(handlers::templates::print),
        )
        .with_state(app_state)
}

/// Background task to clean up expired cache entries.
async fn cleanup_caches(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    let expiration = Duration::from_secs(SESSION_EXPIRATION_SECS);

    loop {
        interval.tick().await;
        let now = Instant::now();

        // Clean up intensity cache
        {
            let mut cache = state.intensity_cache.write().await;
            let before = cache.len();
            cache.retain(|_, v| now.duration_since(v.last_accessed) < expiration);
            let after = cache.len();
            if before != after {
                println!(
                    "[cache] Cleaned up {} expired intensity cache entries ({} remaining)",
                    before - after,
                    after
                );
            }
        }

        // Clean up photo sessions
        {
            let mut sessions = state.photo_sessions.write().await;
            let before = sessions.len();
            sessions.retain(|_, v| now.duration_since(v.last_accessed) < expiration);
            let after = sessions.len();
            if before != after {
                println!(
                    "[cache] Cleaned up {} expired photo sessions ({} remaining)",
                    before - after,
                    after
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printer::Protocol;
    use crate::transport::{Printer, PrinterTarget};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        app_with(None, "127.0.0.1:1")
    }

    fn app_with(templates_dir: Option<std::path::PathBuf>, printer: &str) -> Router {
        router(Arc::new(AppState::new(ServerConfig {
            printer: Printer {
                protocol: Protocol::EscPos,
                // By default nothing listens here: health must not depend on
                // the printer.
                target: PrinterTarget::Tcp(printer.into()),
            },
            listen_addr: "127.0.0.1:0".into(),
            templates_dir,
        })))
    }

    const TODO_TEMPLATE: &str = r#"{
        "name": "TODO",
        "fields": [
            {"name": "title", "type": "text", "required": true},
            {"name": "url", "type": "url"}
        ],
        "document": [
            {"type": "text", "content": "TODO: {{title}}"},
            {"type": "qr_code", "data": "{{url}}", "when": "url"}
        ]
    }"#;

    struct TemplatesDir(std::path::PathBuf);

    impl TemplatesDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("estrella-api-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("todo.json"), TODO_TEMPLATE).unwrap();
            std::fs::write(dir.join("broken.json"), r#"{"document": "nope"}"#).unwrap();
            TemplatesDir(dir)
        }
    }

    impl Drop for TemplatesDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, body.to_vec())
    }

    fn post_json(uri: &str, body: &str) -> Request<Body> {
        Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn json(body: &[u8]) -> serde_json::Value {
        serde_json::from_slice(body).unwrap()
    }

    #[tokio::test]
    async fn templates_list_is_empty_when_not_configured() {
        let (status, body) = send(
            app(),
            Request::get("/api/templates").body(Body::empty()).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body)["configured"], false);
    }

    #[tokio::test]
    async fn templates_list_shows_fields_and_broken_templates() {
        let dir = TemplatesDir::new();

        let (status, body) = send(
            app_with(Some(dir.0.clone()), "127.0.0.1:1"),
            Request::get("/api/templates").body(Body::empty()).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let list = json(&body);
        assert_eq!(list["templates"][0]["id"], "todo");
        assert_eq!(list["templates"][0]["fields"][1]["type"], "url");
        assert_eq!(list["broken"][0]["id"], "broken");
    }

    #[tokio::test]
    async fn template_preview_renders_a_png() {
        let dir = TemplatesDir::new();

        let (status, body) = send(
            app_with(Some(dir.0.clone()), "127.0.0.1:1"),
            post_json(
                "/api/templates/todo/preview",
                r#"{"values": {"title": "Milk"}}"#,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.starts_with(b"\x89PNG"));
    }

    #[tokio::test]
    async fn template_preview_reports_invalid_values_per_field() {
        let dir = TemplatesDir::new();

        let (status, body) = send(
            app_with(Some(dir.0.clone()), "127.0.0.1:1"),
            post_json(
                "/api/templates/todo/preview",
                r#"{"values": {"url": "not a url"}}"#,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        let errors = &json(&body)["errors"];
        assert_eq!(errors["title"], "required");
        assert_eq!(errors["url"], "must be an http:// or https:// URL");
    }

    #[tokio::test]
    async fn unknown_and_broken_templates_are_errors() {
        let dir = TemplatesDir::new();
        let app = app_with(Some(dir.0.clone()), "127.0.0.1:1");

        let (status, _) = send(app.clone(), post_json("/api/templates/nope/preview", "{}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = send(
            app.clone(),
            post_json("/api/templates/..%2Ftodo/preview", "{}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, body) = send(app, post_json("/api/templates/broken/preview", "{}")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(json(&body)["error"].as_str().unwrap().contains("broken"));
    }

    #[tokio::test]
    async fn template_print_sends_the_filled_document() {
        let dir = TemplatesDir::new();
        let printer = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = printer.local_addr().unwrap().to_string();
        let received = std::thread::spawn(move || {
            use std::io::Read;
            let (mut socket, _) = printer.accept().unwrap();
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            bytes
        });

        let (status, body) = send(
            app_with(Some(dir.0.clone()), &address),
            post_json(
                "/api/templates/todo/print",
                r#"{"values": {"title": "Milk", "url": "https://example.com/milk"}}"#,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert_eq!(json(&body)["success"], true);
        let bytes = received.join().unwrap();
        let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(contains(b"TODO: Milk"));
        assert!(contains(b"https://example.com/milk"));
    }

    #[tokio::test]
    async fn healthz_is_ok_without_a_reachable_printer() {
        let response = app()
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
