#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kyberia_desktop_lib::{
    CreateBlankProjectRequest, DesktopIpcError, DesktopState, JobCancellation,
    OpenProjectGrantRequest, OpenProjectSelectionResponse, SchemaRequest, SelectOpenProjectRequest,
    begin_job, begin_open_job, blank_project_path, cancel_job, cancelled_error, error,
    finish_joined_job, issue_open_grant, response_for_with_cancel, run_atomic_project_step,
};
use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

fn lock_state<'a>(
    state: &'a State<'_, Mutex<DesktopState>>,
) -> Result<std::sync::MutexGuard<'a, DesktopState>, DesktopIpcError> {
    state.lock().map_err(|_| {
        error(
            "storage",
            "The desktop project session is unavailable.",
            Some("Restart RF Atlas."),
            true,
        )
    })
}

fn native_project_selection() -> Result<Option<PathBuf>, DesktopIpcError> {
    #[cfg(target_os = "macos")]
    let output = Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Open RF Atlas project\")",
        ])
        .output();
    #[cfg(target_os = "windows")]
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }",
        ])
        .output();
    #[cfg(target_os = "linux")]
    let output = Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title=Open RF Atlas project",
        ])
        .output()
        .or_else(|_| {
            Command::new("kdialog")
                .args(["--getexistingdirectory", ".", "Open RF Atlas project"])
                .output()
        });
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let output: Result<std::process::Output, std::io::Error> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "native project selection is unavailable on this target",
    ));
    let output = output.map_err(|_| {
        error(
            "capability_unavailable",
            "Native project selection is unavailable on this platform.",
            Some("Choose a project from a supported desktop build."),
            false,
        )
    })?;
    if !output.status.success() {
        return Ok(None);
    }
    let selected = String::from_utf8(output.stdout).map_err(|_| {
        error(
            "invalid_response",
            "The native project selector returned an invalid path.",
            Some("Choose the project again."),
            false,
        )
    })?;
    let selected = selected.trim();
    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(selected)))
}

#[tauri::command]
async fn project_create_blank(
    app: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
    request: CreateBlankProjectRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
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
    let job_id = request.job_id;
    let (control, application) = {
        let mut guard = lock_state(&state)?;
        let control = begin_job(&mut guard, &job_id)?;
        (control, guard.application())
    };
    let name = request.name;
    let task_control = Arc::clone(&control);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut cancellation = JobCancellation::new(task_control);
        cancellation.control().set_progress(5);
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let name = kyberia_domain::identity::Text::new(name).map_err(|value| {
            error(
                "invalid_request",
                value.to_string(),
                Some("Provide a non-empty project name."),
                false,
            )
        })?;
        cancellation.control().set_progress(15);
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let session = run_atomic_project_step(cancellation.control(), || {
            application
                .create(kyberia_application::CreateProject {
                    path,
                    name,
                    created_utc_ms: now,
                })
                .map_err(DesktopIpcError::from)
        })?;
        cancellation.control().set_progress(65);
        let response = response_for_with_cancel(&session, &mut cancellation)?;
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        cancellation.control().set_progress(100);
        Ok((session, response))
    })
    .await;
    let mut guard = lock_state(&state)?;
    let result = finish_joined_job(&mut guard, &job_id, result)?;
    match result {
        Ok((session, response)) => {
            guard.set_session(session);
            Ok(response)
        }
        Err(value) => Err(value),
    }
}

#[tauri::command]
fn project_select_open(
    state: State<'_, Mutex<DesktopState>>,
    request: SelectOpenProjectRequest,
) -> Result<OpenProjectSelectionResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let selected = native_project_selection()?;
    let Some(selected) = selected else {
        return Ok(OpenProjectSelectionResponse {
            schema: kyberia_desktop_lib::IPC_SCHEMA,
            selection: None,
        });
    };
    let mut guard = lock_state(&state)?;
    issue_open_grant(&mut guard, selected)
}

#[tauri::command]
async fn project_open_grant(
    state: State<'_, Mutex<DesktopState>>,
    request: OpenProjectGrantRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
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
    let job_id = request.job_id;
    let (control, application, path) = {
        let mut guard = lock_state(&state)?;
        let (control, path) = begin_open_job(
            &mut guard,
            &job_id,
            &request.grant_id,
            request.expected_name.as_deref(),
        )?;
        (control, guard.application(), path)
    };
    let task_control = Arc::clone(&control);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut cancellation = JobCancellation::new(task_control);
        cancellation.control().set_progress(5);
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        cancellation.control().set_progress(15);
        let session = run_atomic_project_step(cancellation.control(), || {
            application
                .open(kyberia_application::OpenProject { path, mode })
                .map_err(DesktopIpcError::from)
        })?;
        cancellation.control().set_progress(65);
        let response = response_for_with_cancel(&session, &mut cancellation)?;
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        cancellation.control().set_progress(100);
        Ok((session, response))
    })
    .await;
    let mut guard = lock_state(&state)?;
    let result = finish_joined_job(&mut guard, &job_id, result)?;
    match result {
        Ok((session, response)) => {
            guard.set_session(session);
            Ok(response)
        }
        Err(value) => Err(value),
    }
}

#[tauri::command]
async fn project_current(
    state: State<'_, Mutex<DesktopState>>,
    request: SchemaRequest,
) -> Result<kyberia_desktop_lib::CurrentProjectResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let job_id = request.job_id;
    let (control, session) = {
        let mut guard = lock_state(&state)?;
        let Some(session) = guard.take_session() else {
            return Ok(kyberia_desktop_lib::no_project_response());
        };
        let control = match begin_job(&mut guard, &job_id) {
            Ok(value) => value,
            Err(error) => {
                guard.set_session(session);
                return Err(error);
            }
        };
        (control, session)
    };
    let task_control = Arc::clone(&control);
    let task_result = tauri::async_runtime::spawn_blocking(move || {
        let mut cancellation = JobCancellation::new(task_control);
        cancellation.control().set_progress(25);
        let result = response_for_with_cancel(&session, &mut cancellation).and_then(|response| {
            if cancellation.is_cancelled() {
                Err(cancelled_error())
            } else {
                cancellation.control().set_progress(100);
                Ok(response)
            }
        });
        (session, result)
    })
    .await;
    let mut guard = lock_state(&state)?;
    let task_result = finish_joined_job(&mut guard, &job_id, task_result)?;
    match task_result {
        (session, Ok(response)) => {
            guard.set_session(session);
            Ok(response)
        }
        (session, Err(value)) => {
            guard.set_session(session);
            Err(value)
        }
    }
}

#[tauri::command]
fn project_cancel(
    state: State<'_, Mutex<DesktopState>>,
    request: kyberia_desktop_lib::JobRequest,
) -> Result<kyberia_desktop_lib::JobCancelResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let guard = lock_state(&state)?;
    cancel_job(&guard, &request.job_id)?;
    Ok(kyberia_desktop_lib::JobCancelResponse {
        schema: kyberia_desktop_lib::IPC_SCHEMA,
        job_id: request.job_id,
        state: "cancelling",
    })
}

#[tauri::command]
fn project_job_status(
    state: State<'_, Mutex<DesktopState>>,
    request: kyberia_desktop_lib::JobRequest,
) -> Result<kyberia_desktop_lib::JobStatusResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let guard = lock_state(&state)?;
    let Some(control) = kyberia_desktop_lib::job_control(&guard, &request.job_id) else {
        return Err(error(
            "invalid_request",
            "That project operation is no longer running.",
            Some("Refresh the project state."),
            false,
        ));
    };
    Ok(kyberia_desktop_lib::JobStatusResponse {
        schema: kyberia_desktop_lib::IPC_SCHEMA,
        job_id: request.job_id,
        state: if control.is_cancelled() {
            "cancelling"
        } else {
            "running"
        },
        progress: control.progress(),
    })
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(DesktopState::default()))
        .invoke_handler(tauri::generate_handler![
            project_create_blank,
            project_select_open,
            project_open_grant,
            project_current,
            project_cancel,
            project_job_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running RF Atlas desktop application");
}
