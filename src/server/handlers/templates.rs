//! Template handlers: list templates, and preview or print one filled with
//! form values.
//!
//! Templates are re-read from the templates directory on every request, so
//! edits show up without a restart.

use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use super::super::state::AppState;
use super::json_api::{print_document, render_preview};
use crate::document::Document;
use crate::templates::{BrokenTemplate, FillError, Template, TemplateLibrary};

#[derive(Serialize)]
pub struct TemplateList {
    /// Whether a templates directory is configured.
    configured: bool,
    templates: Vec<Template>,
    broken: Vec<BrokenTemplate>,
    /// Why the directory couldn't be read, if it couldn't.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct FillRequest {
    #[serde(default)]
    values: HashMap<String, String>,
}

/// Handle GET /api/templates.
pub async fn list(State(state): State<Arc<AppState>>) -> Json<TemplateList> {
    let Some(dir) = state.config.templates_dir.clone() else {
        return Json(TemplateList {
            configured: false,
            templates: Vec::new(),
            broken: Vec::new(),
            error: None,
        });
    };
    let loaded = tokio::task::spawn_blocking(move || TemplateLibrary::load(&dir))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    Json(match loaded {
        Ok(library) => TemplateList {
            configured: true,
            templates: library.templates,
            broken: library.broken,
            error: None,
        },
        Err(error) => TemplateList {
            configured: true,
            templates: Vec::new(),
            broken: Vec::new(),
            error: Some(error),
        },
    })
}

/// Handle POST /api/templates/{id}/preview - render the filled template as PNG.
pub async fn preview(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<FillRequest>,
) -> Response {
    let doc = match fill(&state, &id, &request.values).await {
        Ok(doc) => doc,
        Err(response) => return *response,
    };
    match render_preview(&state, doc).await {
        Ok(png) => ([(header::CONTENT_TYPE, "image/png")], png).into_response(),
        Err((status, error)) => error_response(status, error),
    }
}

/// Handle POST /api/templates/{id}/print - print the filled template.
pub async fn print(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<FillRequest>,
) -> Response {
    let doc = match fill(&state, &id, &request.values).await {
        Ok(doc) => doc,
        Err(response) => return *response,
    };
    match print_document(&state, doc).await {
        Ok(()) => Json(json!({"success": true, "message": "Printed"})).into_response(),
        Err((status, error)) => error_response(status, error),
    }
}

/// Load template `id` and fill it, or build the error response.
async fn fill(
    state: &AppState,
    id: &str,
    values: &HashMap<String, String>,
) -> Result<Document, Box<Response>> {
    let Some(dir) = state.config.templates_dir.clone() else {
        return Err(Box::new(error_response(
            StatusCode::NOT_FOUND,
            "No templates directory is configured".into(),
        )));
    };
    let library = tokio::task::spawn_blocking(move || TemplateLibrary::load(&dir))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
        .map_err(|e| Box::new(error_response(StatusCode::INTERNAL_SERVER_ERROR, e)))?;

    let Some(template) = library.get(id) else {
        if let Some(broken) = library.broken.iter().find(|b| b.id == id) {
            return Err(Box::new(error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Template '{id}' is broken: {}", broken.error),
            )));
        }
        return Err(Box::new(error_response(
            StatusCode::NOT_FOUND,
            format!("No template '{id}'"),
        )));
    };

    template.fill(values).map_err(|e| {
        Box::new(match e {
            FillError::Invalid(errors) => (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({"success": false, "error": "Some values are invalid", "errors": errors}),
                ),
            )
                .into_response(),
            FillError::Document(error) => error_response(StatusCode::UNPROCESSABLE_ENTITY, error),
        })
    })
}

fn error_response(status: StatusCode, error: String) -> Response {
    (status, Json(json!({"success": false, "error": error}))).into_response()
}
