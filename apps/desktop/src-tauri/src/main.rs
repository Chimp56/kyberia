#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kyberia_desktop_lib::{
    CreateBlankProjectRequest, CreateProjectRequest, DesktopIpcError, DesktopState,
    OpenProjectRequest, blank_project_path, create_project_at, current_project, error,
    require_schema,
};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

#[tauri::command]
fn project_create_blank(
    app: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
    request: CreateBlankProjectRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    require_schema(&request.schema)?;
    let app_data_dir = app.path().app_data_dir().map_err(|value| {
        error(
            "storage",
            format!("Could not locate RF Atlas application data: {value}"),
            Some("Choose a writable application data directory."),
            true,
        )
    })?;
    let path = blank_project_path(app_data_dir)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|value| {
            error(
                "storage",
                format!("System clock is before the Unix epoch: {value}"),
                None,
                true,
            )
        })?
        .as_millis() as i64;
    let mut guard = state.lock().map_err(|_| {
        error(
            "storage",
            "The desktop project session is unavailable.",
            Some("Restart RF Atlas."),
            true,
        )
    })?;
    create_project_at(&mut guard, path, request.name, now)
}

#[tauri::command]
fn project_create(
    state: State<'_, Mutex<DesktopState>>,
    request: CreateProjectRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    require_schema(&request.schema)?;
    let mut guard = state.lock().map_err(|_| {
        error(
            "storage",
            "The desktop project session is unavailable.",
            Some("Restart RF Atlas."),
            true,
        )
    })?;
    create_project_at(
        &mut guard,
        PathBuf::from(request.path),
        request.name,
        request.created_utc_ms,
    )
}

#[tauri::command]
fn project_open(
    state: State<'_, Mutex<DesktopState>>,
    request: OpenProjectRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    require_schema(&request.schema)?;
    let mode = match request.mode.as_str() {
        "read_only" => kyberia_application::SessionMode::ReadOnly,
        "read_write" => kyberia_application::SessionMode::ReadWrite,
        _ => {
            return Err(error(
                "invalid_request",
                "Unknown project open mode.",
                Some("Use read_only or read_write."),
                false,
            ));
        }
    };
    let mut guard = state.lock().map_err(|_| {
        error(
            "storage",
            "The desktop project session is unavailable.",
            Some("Restart RF Atlas."),
            true,
        )
    })?;
    kyberia_desktop_lib::open_project_at(&mut guard, PathBuf::from(request.path), mode)
}

#[tauri::command]
fn project_current(
    state: State<'_, Mutex<DesktopState>>,
    request: kyberia_desktop_lib::SchemaRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    require_schema(&request.schema)?;
    let guard = state.lock().map_err(|_| {
        error(
            "storage",
            "The desktop project session is unavailable.",
            Some("Restart RF Atlas."),
            true,
        )
    })?;
    current_project(&guard)
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(DesktopState::default()))
        .invoke_handler(tauri::generate_handler![
            project_create_blank,
            project_create,
            project_open,
            project_current
        ])
        .run(tauri::generate_context!())
        .expect("error while running RF Atlas desktop application");
}
