//! Outward file-handle validation. No platform handle enters the domain.
use std::{fs::File, io};

/// Require a disk-backed handle before callers attempt blocking file reads.
/// This is a handle-kind check, not a read deadline or a path sandbox.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn require_disk_file(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_DISK, GetFileType};
    // SAFETY: `file` owns a valid handle for the duration of this shared borrow.
    // GetFileType only queries it; it neither closes nor retains the handle and
    // accepts handles opened without additional access rights. No raw pointer
    // is dereferenced by Rust or returned to the caller.
    let kind = unsafe { GetFileType(file.as_raw_handle()) };
    if kind != FILE_TYPE_DISK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "request handle is not a disk file",
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn require_disk_file(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows disk-handle validation is unavailable",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn rejects_null_character_device() {
        let file = File::open("NUL").unwrap();
        assert_eq!(
            require_disk_file(&file).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    #[test]
    fn rejects_a_connected_named_pipe_before_any_read() {
        use std::{
            process::{Child, Command, Stdio},
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };
        struct Server(Child);
        impl Drop for Server {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let name = format!(
            "kyberia-handle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        // An original local test fixture. No capture, credentials, network or
        // filesystem deletion is involved; the server deliberately sends no data.
        let script = "$p = [System.IO.Pipes.NamedPipeServerStream]::new($env:KYBERIA_TEST_PIPE, [System.IO.Pipes.PipeDirection]::Out); $p.WaitForConnection(); Start-Sleep -Seconds 30; $p.Dispose()";
        let mut server = Server(
            Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .env("KYBERIA_TEST_PIPE", &name)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Windows test requires the system PowerShell pipe fixture"),
        );
        let path = format!(r"\\.\pipe\{name}");
        let started = Instant::now();
        let file = loop {
            match File::open(&path) {
                Ok(file) => break file,
                Err(error) => {
                    assert!(
                        started.elapsed() < Duration::from_secs(10),
                        "pipe fixture did not start: {error}"
                    );
                    assert!(
                        server.0.try_wait().unwrap().is_none(),
                        "pipe fixture exited early"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        assert_eq!(
            require_disk_file(&file).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn accepts_opened_executable_disk_file() {
        let file = File::open(std::env::current_exe().unwrap()).unwrap();
        require_disk_file(&file).unwrap();
    }
}
