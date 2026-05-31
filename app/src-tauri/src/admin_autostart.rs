//! Elevated autostart through the Windows Task Scheduler.
//!
//! `tauri-plugin-autostart` registers a normal (non-elevated) login item via
//! the `Run` registry key. That cannot launch the app *as administrator*,
//! which some users want so the elevated DDC/CI paths and the fastest possible
//! start are available right at sign-in.
//!
//! This module registers a Task Scheduler task whose principal runs with the
//! **highest available** privileges, triggered at logon. Creating or deleting
//! such a task itself requires elevation, so the `schtasks.exe` invocation is
//! launched through `ShellExecuteExW` with the `runas` verb — Windows shows a
//! single UAC prompt. Querying the task does not need elevation and runs
//! silently.
//!
//! On non-Windows targets every function returns
//! [`brightness_core`-style] "unsupported" errors.

/// Stable Task Scheduler task name.
#[cfg(windows)]
const TASK_NAME: &str = "MonitorBrightnessControl-AdminAutostart";

/// Returns whether the elevated autostart task currently exists.
#[cfg(windows)]
pub fn status() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // CREATE_NO_WINDOW so no console flashes on screen.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("schtasks query failed to start: {e}"))?;
    Ok(out.status.success())
}

/// Enable or disable the elevated autostart task. Enabling writes a task XML
/// and registers it elevated; disabling deletes it elevated. Both elevated
/// operations trigger a UAC prompt.
#[cfg(windows)]
pub fn set(enabled: bool) -> Result<(), String> {
    if enabled {
        create_task()
    } else {
        delete_task()
    }
}

#[cfg(windows)]
fn create_task() -> Result<(), String> {
    use std::io::Write;

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe_str = exe.to_string_lossy().to_string();

    // Task Scheduler 1.2 XML. Using XML (rather than the /TR command line)
    // keeps the executable path and arguments cleanly separated, avoiding all
    // quoting pitfalls with spaces in the path.
    let xml = build_task_xml(&exe_str);

    // Write the XML as UTF-16LE with BOM — schtasks expects that for /XML.
    let dir = std::env::temp_dir();
    let xml_path = dir.join(format!("{TASK_NAME}.xml"));
    {
        let mut f = std::fs::File::create(&xml_path)
            .map_err(|e| format!("create task xml: {e}"))?;
        let mut bytes = Vec::with_capacity(xml.len() * 2 + 2);
        bytes.extend_from_slice(&[0xFF, 0xFE]); // UTF-16LE BOM
        for unit in xml.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        f.write_all(&bytes).map_err(|e| format!("write task xml: {e}"))?;
    }

    let params = format!(
        "/Create /TN \"{TASK_NAME}\" /XML \"{}\" /F",
        xml_path.to_string_lossy()
    );
    let code = run_elevated("schtasks.exe", &params)?;
    // Best-effort cleanup of the temp file.
    let _ = std::fs::remove_file(&xml_path);

    if code != 0 {
        return Err(format!(
            "schtasks /Create exited with code {code} (was the UAC prompt cancelled?)"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn delete_task() -> Result<(), String> {
    let params = format!("/Delete /TN \"{TASK_NAME}\" /F");
    let code = run_elevated("schtasks.exe", &params)?;
    if code != 0 {
        return Err(format!(
            "schtasks /Delete exited with code {code} (was the UAC prompt cancelled?)"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn build_task_xml(exe_path: &str) -> String {
    // Escape XML special characters in the path.
    let cmd = xml_escape(exe_path);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Monitor Brightness Control — launch at sign-in with the highest available privileges.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{cmd}</Command>
      <Arguments>--minimized</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Run `program params` elevated via `ShellExecuteExW(runas)`, wait for it,
/// and return its exit code. A code of 0 means success; non-zero (including
/// the UAC cancel which surfaces as an `ShellExecuteEx` failure) is reported.
#[cfg(windows)]
fn run_elevated(program: &str, params: &str) -> Result<u32, String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let program_w = HSTRING::from(program);
    let params_w = HSTRING::from(params);

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(program_w.as_ptr()),
        lpParameters: PCWSTR(params_w.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    // SAFETY: `info` is fully initialized; the string pointers outlive the
    // call. ShellExecuteExW fills `hProcess` because of SEE_MASK_NOCLOSEPROCESS.
    unsafe {
        ShellExecuteExW(&mut info)
            .map_err(|e| format!("elevation request failed or was declined: {e}"))?;
    }

    let process: HANDLE = info.hProcess;
    if process.is_invalid() {
        return Err("elevated process handle was not returned".into());
    }

    // SAFETY: `process` is a valid handle owned by us until CloseHandle.
    let exit_code = unsafe {
        let wait = WaitForSingleObject(process, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let _ = CloseHandle(process);
            return Err("waiting for elevated schtasks failed".into());
        }
        let mut code: u32 = 0;
        let got = GetExitCodeProcess(process, &mut code);
        let _ = CloseHandle(process);
        got.map_err(|e| format!("GetExitCodeProcess: {e}"))?;
        code
    };
    Ok(exit_code)
}

// ---------------------------------------------------------------------------
// Non-Windows stubs
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
pub fn status() -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
pub fn set(_enabled: bool) -> Result<(), String> {
    Err("administrator autostart via Task Scheduler is only available on Windows".into())
}
