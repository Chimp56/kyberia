#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kyberia_desktop_lib::{
    CreateBlankProjectRequest, DesktopIpcError, DesktopState, JobCancellation,
    OpenProjectGrantRequest, OpenProjectSelectionResponse, SchemaRequest, SelectOpenProjectRequest,
    begin_job, begin_open_job, blank_project_path, cancel_created_project, cancel_job,
    cancelled_error, current_project_job, error, finish_created_project_job, finish_joined_job,
    issue_open_grant, response_for_with_cancel, retain_cancelled_project, run_atomic_project_step,
};
use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

const NATIVE_PICKER_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
struct PickerTerminationEvidence {
    termination_requested: bool,
    wait_status: Option<std::process::ExitStatus>,
}

fn lock_state<'a>(
    state: &'a State<'_, Mutex<DesktopState>>,
) -> Result<std::sync::MutexGuard<'a, DesktopState>, DesktopIpcError> {
    lock_state_ref(state.inner())
}

fn lock_state_ref<'a>(
    state: &'a Mutex<DesktopState>,
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

fn terminate_and_reap_picker(child: &mut Child) -> PickerTerminationEvidence {
    let mut termination_requested = false;
    #[cfg(target_os = "windows")]
    {
        let taskkill = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("taskkill.exe");
        termination_requested = Command::new(taskkill)
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .status()
            .is_ok_and(|status| status.success());
    }
    termination_requested |= child.kill().is_ok();
    let wait_status = child.wait().ok();
    PickerTerminationEvidence {
        termination_requested,
        wait_status,
    }
}

fn picker_reap_error() -> DesktopIpcError {
    error(
        "storage",
        "The native project selector could not be reaped safely.",
        Some("Restart RF Atlas before choosing a project again."),
        true,
    )
}

fn run_owned_picker(
    mut command: Command,
    control: &kyberia_desktop_lib::JobControl,
    timeout: Duration,
) -> Result<Output, DesktopIpcError> {
    if control.is_cancelled() {
        return Err(cancelled_error());
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            error(
                "capability_unavailable",
                "Native project selection is unavailable on this platform.",
                Some("Choose a project from a supported desktop build."),
                false,
            )
        })?;
    let started = Instant::now();
    loop {
        if control.is_cancelled() {
            let evidence = terminate_and_reap_picker(&mut child);
            if !evidence.termination_requested || evidence.wait_status.is_none() {
                return Err(picker_reap_error());
            }
            return Err(cancelled_error());
        }
        if started.elapsed() >= timeout {
            let evidence = terminate_and_reap_picker(&mut child);
            if !evidence.termination_requested || evidence.wait_status.is_none() {
                return Err(picker_reap_error());
            }
            return Err(error(
                "timeout",
                "Native project selection timed out.",
                Some("Choose the project again."),
                true,
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                child
                    .stdout
                    .take()
                    .ok_or_else(|| {
                        error(
                            "capability_unavailable",
                            "Native project selection could not be read.",
                            Some("Choose the project again."),
                            true,
                        )
                    })?
                    .read_to_end(&mut stdout)
                    .map_err(|_| {
                        error(
                            "capability_unavailable",
                            "Native project selection could not be read.",
                            Some("Choose the project again."),
                            true,
                        )
                    })?;
                return Ok(Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let evidence = terminate_and_reap_picker(&mut child);
                if !evidence.termination_requested || evidence.wait_status.is_none() {
                    return Err(picker_reap_error());
                }
                return Err(error(
                    "capability_unavailable",
                    "Native project selection could not be monitored.",
                    Some("Choose a project from a supported desktop build."),
                    false,
                ));
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn powershell_path() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

fn native_project_selection(
    control: &kyberia_desktop_lib::JobControl,
) -> Result<Option<PathBuf>, DesktopIpcError> {
    #[cfg(target_os = "macos")]
    let output = {
        let mut command = Command::new("/usr/bin/osascript");
        command.args([
            "-e",
            "POSIX path of (choose folder with prompt \"Open RF Atlas project\")",
        ]);
        run_owned_picker(command, control, NATIVE_PICKER_TIMEOUT)
    };
    #[cfg(target_os = "windows")]
    let output = {
        let mut command = Command::new(powershell_path());
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; if ($dialog.ShowDialog() -eq 'OK') { $dialog.SelectedPath }",
        ]);
        run_owned_picker(command, control, NATIVE_PICKER_TIMEOUT)
    };
    #[cfg(target_os = "linux")]
    let output = {
        let mut zenity = Command::new("/usr/bin/zenity");
        zenity.args([
            "--file-selection",
            "--directory",
            "--title=Open RF Atlas project",
        ]);
        match run_owned_picker(zenity, control, NATIVE_PICKER_TIMEOUT) {
            Err(value) if value.code == "capability_unavailable" => {
                let mut kdialog = Command::new("/usr/bin/kdialog");
                kdialog.args(["--getexistingdirectory", ".", "Open RF Atlas project"]);
                run_owned_picker(kdialog, control, NATIVE_PICKER_TIMEOUT)
            }
            other => other,
        }
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let output: Result<Output, DesktopIpcError> = Err(error(
        "capability_unavailable",
        "Native project selection is unavailable on this platform.",
        Some("Choose a project from a supported desktop build."),
        false,
    ));
    let output = output?;
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
    let cleanup_path = path.clone();
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
        let session = match application.create(kyberia_application::CreateProject {
            path: path.clone(),
            name,
            created_utc_ms: now,
        }) {
            Ok(session) => session,
            Err(_value) if cancellation.is_cancelled() => {
                retain_cancelled_project(&path)?;
                return Err(cancelled_error());
            }
            Err(value) => return Err(DesktopIpcError::from(value)),
        };
        cancellation.control().set_progress(65);
        let response = match response_for_with_cancel(&session, &mut cancellation) {
            Ok(response) => response,
            Err(value) if cancellation.is_cancelled() => {
                drop(session);
                cancel_created_project(&path, cancellation.control())?;
                return Err(value);
            }
            Err(value) => return Err(value),
        };
        if cancellation.is_cancelled() {
            drop(session);
            cancel_created_project(&path, cancellation.control())?;
            return Err(cancelled_error());
        }
        cancellation.control().set_progress(100);
        Ok((session, response))
    })
    .await;
    let mut guard = lock_state(&state)?;
    let result = finish_created_project_job(&mut guard, &job_id, &cleanup_path, result)?;
    match result {
        Ok((session, response)) => {
            if control.is_cancelled() {
                drop(session);
                retain_cancelled_project(&cleanup_path)?;
                return Err(cancelled_error());
            }
            guard.set_session(session);
            Ok(response)
        }
        Err(value) => Err(value),
    }
}

async fn project_select_open_with_picker<F>(
    state: &Mutex<DesktopState>,
    request: SelectOpenProjectRequest,
    picker: F,
) -> Result<OpenProjectSelectionResponse, DesktopIpcError>
where
    F: FnOnce(Arc<kyberia_desktop_lib::JobControl>) -> Result<Option<PathBuf>, DesktopIpcError>
        + Send
        + 'static,
{
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let job_id = request.job_id;
    let control = {
        let mut guard = lock_state_ref(state)?;
        begin_job(&mut guard, &job_id)?
    };
    control.set_progress(5);
    let task_control = Arc::clone(&control);
    let joined = tauri::async_runtime::spawn_blocking(move || picker(task_control)).await;
    let mut guard = lock_state_ref(state)?;
    let selected = finish_joined_job(&mut guard, &job_id, joined)?;
    if control.is_cancelled() {
        return Err(cancelled_error());
    }
    match selected? {
        Some(selected) => {
            if control.is_cancelled() {
                Err(cancelled_error())
            } else {
                issue_open_grant(&mut guard, selected)
            }
        }
        None => Ok(OpenProjectSelectionResponse {
            schema: kyberia_desktop_lib::IPC_SCHEMA,
            selection: None,
        }),
    }
}

#[tauri::command]
async fn project_select_open(
    state: State<'_, Mutex<DesktopState>>,
    request: SelectOpenProjectRequest,
) -> Result<OpenProjectSelectionResponse, DesktopIpcError> {
    project_select_open_with_picker(state.inner(), request, |control| {
        native_project_selection(&control)
    })
    .await
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
            if control.is_cancelled() {
                drop(session);
                return Err(cancelled_error());
            }
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
    current_project_job(&state, request.job_id).await
}

fn project_cancel_with_state(
    state: &Mutex<DesktopState>,
    request: kyberia_desktop_lib::JobRequest,
) -> Result<kyberia_desktop_lib::JobCancelResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let guard = lock_state_ref(state)?;
    cancel_job(&guard, &request.job_id)?;
    Ok(kyberia_desktop_lib::JobCancelResponse {
        schema: kyberia_desktop_lib::IPC_SCHEMA,
        job_id: request.job_id,
        state: "cancelling",
    })
}

#[tauri::command]
fn project_cancel(
    state: State<'_, Mutex<DesktopState>>,
    request: kyberia_desktop_lib::JobRequest,
) -> Result<kyberia_desktop_lib::JobCancelResponse, DesktopIpcError> {
    project_cancel_with_state(state.inner(), request)
}

fn project_job_status_with_state(
    state: &Mutex<DesktopState>,
    request: kyberia_desktop_lib::JobRequest,
) -> Result<kyberia_desktop_lib::JobStatusResponse, DesktopIpcError> {
    kyberia_desktop_lib::require_schema(&request.schema)?;
    let guard = lock_state_ref(state)?;
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

#[tauri::command]
fn project_job_status(
    state: State<'_, Mutex<DesktopState>>,
    request: kyberia_desktop_lib::JobRequest,
) -> Result<kyberia_desktop_lib::JobStatusResponse, DesktopIpcError> {
    project_job_status_with_state(state.inner(), request)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn short_picker_command() -> Command {
        #[cfg(unix)]
        {
            let mut command = Command::new("/usr/bin/printf");
            command.arg("ready");
            command
        }
        #[cfg(windows)]
        {
            let mut command = Command::new(powershell_path());
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Write-Output ready",
            ]);
            command
        }
    }

    fn long_picker_command() -> Command {
        #[cfg(unix)]
        {
            let mut command = Command::new("/bin/sleep");
            command.arg("10");
            command
        }
        #[cfg(windows)]
        {
            let mut command = Command::new(powershell_path());
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 10",
            ]);
            command
        }
    }

    #[test]
    fn owned_picker_collects_child_output_after_exit() {
        let control = kyberia_desktop_lib::JobControl::new();
        let output = run_owned_picker(short_picker_command(), &control, Duration::from_secs(1))
            .expect("picker output");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ready");
    }

    #[test]
    fn picker_termination_records_kill_and_wait_evidence() {
        let mut command = long_picker_command();
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("long picker child");
        let evidence = terminate_and_reap_picker(&mut child);
        assert!(evidence.termination_requested);
        assert!(evidence.wait_status.is_some());
    }

    #[test]
    fn async_picker_command_state_path_accepts_live_status_and_cancel() {
        use std::sync::mpsc;

        let state = Arc::new(Mutex::new(DesktopState::default()));
        let state_for_picker = Arc::clone(&state);
        let job_id = uuid::Uuid::new_v4().to_string();
        let request = SelectOpenProjectRequest {
            schema: kyberia_desktop_lib::IPC_SCHEMA.to_owned(),
            job_id: job_id.clone(),
        };
        let (live_tx, live_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            tauri::async_runtime::block_on(project_select_open_with_picker(
                &state_for_picker,
                request,
                move |control| {
                    live_tx.send(()).expect("picker worker entered");
                    run_owned_picker(long_picker_command(), &control, Duration::from_secs(2))
                        .map(|_| None)
                },
            ))
        });
        live_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("picker worker is live");

        let status = project_job_status_with_state(
            &state,
            kyberia_desktop_lib::JobRequest {
                schema: kyberia_desktop_lib::IPC_SCHEMA.to_owned(),
                job_id: job_id.clone(),
            },
        )
        .expect("status while picker is live");
        assert_eq!(status.state, "running");
        assert_eq!(status.progress, 5);
        let cancellation = project_cancel_with_state(
            &state,
            kyberia_desktop_lib::JobRequest {
                schema: kyberia_desktop_lib::IPC_SCHEMA.to_owned(),
                job_id: job_id.clone(),
            },
        )
        .expect("cancel while picker is live");
        assert_eq!(cancellation.state, "cancelling");
        let result = worker
            .join()
            .expect("picker command join")
            .expect_err("picker command cancellation");
        assert_eq!(result.code, "cancelled");
        assert!(
            kyberia_desktop_lib::job_control(&state.lock().expect("state after picker"), &job_id,)
                .is_none()
        );
    }

    #[test]
    fn owned_picker_cancellation_terminates_and_reaps_child() {
        let control = Arc::new(kyberia_desktop_lib::JobControl::new());
        let canceller = Arc::clone(&control);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            canceller.cancel();
        });
        let started = Instant::now();
        let result = run_owned_picker(long_picker_command(), &control, Duration::from_secs(2))
            .expect_err("cancelled picker");
        thread.join().expect("cancellation thread");
        assert_eq!(result.code, "cancelled");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn owned_picker_timeout_terminates_and_reaps_child() {
        let control = kyberia_desktop_lib::JobControl::new();
        let started = Instant::now();
        let result = run_owned_picker(long_picker_command(), &control, Duration::from_millis(40))
            .expect_err("timed out picker");
        assert_eq!(result.code, "timeout");
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
