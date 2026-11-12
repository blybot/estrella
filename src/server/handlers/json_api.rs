//! JSON API handlers for preview and printing.
//!
//! Accepts JSON documents using the unified Document model.

use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::document::canvas::ElementLayout;
use crate::document::{self, Component, Document, ImageResolver};
use crate::ir::{Op, Program};
use crate::preview::{measure_cursor_y, measure_preview};

use super::super::state::AppState;

/// Handle POST /api/json/preview - render JSON document as PNG.
pub async fn preview(
    State(state): State<Arc<AppState>>,
    Json(doc): Json<Document>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let png_bytes = render_preview(&state, doc).await?;
    Ok(([(header::CONTENT_TYPE, "image/png")], png_bytes))
}

/// Resolve a document's images and render its preview PNG.
pub(super) async fn render_preview(
    state: &AppState,
    mut doc: Document,
) -> Result<Vec<u8>, (StatusCode, String)> {
    // Resolve images from URLs before compilation
    let resolver = ImageResolver::new(state.photo_sessions.clone());
    resolver.resolve(&mut doc).await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Image resolution failed: {}", e),
        )
    })?;

    let program = doc.compile();
    program.to_preview_png().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Preview render failed: {}", e),
        )
    })
}

/// Request body for canvas-layout endpoint.
#[derive(Deserialize)]
pub struct CanvasLayoutRequest {
    document: Vec<Component>,
    canvas_index: usize,
    #[serde(default)]
    cut: bool,
}

/// Response for canvas-layout: element bounding boxes + document positioning.
#[derive(Serialize)]
pub struct CanvasLayoutResponse {
    pub width: usize,
    pub height: usize,
    pub y_offset: usize,
    pub document_height: usize,
    pub elements: Vec<ElementLayout>,
}

/// Handle POST /api/json/canvas-layout - compute element bounding boxes for canvas overlay.
pub async fn canvas_layout(
    Json(req): Json<CanvasLayoutRequest>,
) -> Result<Json<CanvasLayoutResponse>, (StatusCode, String)> {
    let canvas_component = req.document.get(req.canvas_index).ok_or((
        StatusCode::BAD_REQUEST,
        format!("Invalid canvas_index: {}", req.canvas_index),
    ))?;

    let canvas = match canvas_component {
        Component::Canvas(c) => c,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Component at canvas_index is not a canvas".to_string(),
            ));
        }
    };

    let layout = canvas.compute_layout();

    // Compute Y offset using cursor position (where the canvas starts in the
    // preview image), and document height using trimmed buffer height (matching
    // the actual preview PNG dimensions).
    let mut prefix_ops = vec![Op::Init, Op::SetCodepage(1)];
    for comp in &req.document[..req.canvas_index] {
        comp.emit(&mut prefix_ops);
    }
    let y_offset = measure_cursor_y(&Program { ops: prefix_ops }).unwrap_or(0);

    let mut all_ops = vec![Op::Init, Op::SetCodepage(1)];
    for comp in &req.document {
        comp.emit(&mut all_ops);
    }
    if req.cut {
        all_ops.push(Op::Cut { partial: true });
    }
    let document_height = measure_preview(&Program { ops: all_ops }).unwrap_or(0);

    Ok(Json(CanvasLayoutResponse {
        width: layout.width,
        height: layout.height,
        y_offset,
        document_height,
        elements: layout.elements,
    }))
}

/// Handle POST /api/json/print - print JSON document to device.
pub async fn print(State(state): State<Arc<AppState>>, Json(doc): Json<Document>) -> Response {
    match print_document(&state, doc).await {
        Ok(()) => (
            StatusCode::OK,
            Html(r#"{"success": true, "message": "Document printed successfully"}"#.to_string()),
        )
            .into_response(),
        Err((status, error)) => (
            status,
            Html(format!(r#"{{"success": false, "error": "{}"}}"#, error)),
        )
            .into_response(),
    }
}

/// Resolve a document's images and print it.
pub(super) async fn print_document(
    state: &AppState,
    mut doc: Document,
) -> Result<(), (StatusCode, String)> {
    // Resolve images from URLs before compilation
    let resolver = ImageResolver::new(state.photo_sessions.clone());
    if let Err(e) = resolver.resolve(&mut doc).await {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Image resolution failed: {}", e),
        ));
    }

    match serde_json::to_string_pretty(&doc) {
        Ok(json) => eprintln!("=== JSON Print ===\n{}\n==================", json),
        Err(e) => eprintln!("(failed to serialize document for logging: {})", e),
    }

    let printer = state.config.printer.clone();

    let print_result = tokio::task::spawn_blocking(move || {
        let print_data = doc.build_for(printer.protocol)?;
        printer.print_bytes(&print_data)
    })
    .await;

    match print_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Print failed: {}", e),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Task error: {}", e),
        )),
    }
}

/// Handle GET /api/json/component/:type/default - return a default component by type name.
pub async fn component_default(
    Path(type_name): Path<String>,
) -> Result<Json<Component>, StatusCode> {
    document::default_component(&type_name)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
