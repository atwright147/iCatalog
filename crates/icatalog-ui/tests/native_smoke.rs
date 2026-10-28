//! Runs only on an explicitly selected, logged-in macOS desktop.
//! System Events is the OS automation interface; the test driver is Rust.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod support;
use icatalog_catalog::APP_NAME;
use icatalog_engine::{dispatch_guarded, Context, Params};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct OwnedProcess(Child);
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn wait_for(description: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "Timed out waiting for {description}; see target/ui-smoke/native-*.log"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn automate(pid: u32, action: &str) {
    let script = format!(
        r#"
        tell application "System Events"
            repeat 200 times
                if exists (first application process whose unix id is {pid}) then exit repeat
                delay 0.05
            end repeat
            set targetProcess to a reference to (first application process whose unix id is {pid})
            if unix id of targetProcess is not {pid} then error "UI automation resolved the wrong process"
            tell targetProcess
                if unix id is not {pid} then error "UI automation resolved the wrong process"
                set frontmost to true
                repeat 200 times
                    if frontmost then exit repeat
                    delay 0.05
                end repeat
                if not frontmost then error "Application did not become frontmost"
                {action}
            end tell
        end tell
    "#
    );
    let mut child = OwnedProcess(
        Command::new("osascript")
            .args(["-e", &script])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut timed_out = false;
    while child.0.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            child.0.kill().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = child.0.wait().unwrap();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.0.stderr.take() {
        use std::io::Read;
        pipe.read_to_string(&mut stderr).unwrap();
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir().join("native-automation.log"))
        .unwrap();
    writeln!(log, "PID {pid}: {status}; timed out: {timed_out}\n{stderr}").unwrap();
    assert!(
        !timed_out,
        "macOS automation timed out (check Accessibility/Automation permissions): {stderr}"
    );
    assert!(status.success(), "macOS automation failed: {stderr}");
}

fn log_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-smoke")
}

fn isolated_executable(root: &Path) -> PathBuf {
    // System Events can conflate unbundled processes with the same executable
    // name, even when the initial lookup uses a PID. Give the production binary
    // its own app identity so another running iCatalog cannot receive actions.
    let contents = root.join("iCatalog.app/Contents");
    let executable_name = format!("icatalog-ui-native-smoke-{}", std::process::id());
    let executable = contents.join("MacOS").join(&executable_name);
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_icatalog-ui"), &executable).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleName</key><string>{APP_NAME}</string>
<key>CFBundleDisplayName</key><string>{APP_NAME}</string>
<key>CFBundleIdentifier</key><string>org.icatalog.native-smoke.p{}</string>
<key>CFBundleExecutable</key><string>{executable_name}</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>"#,
            std::process::id()
        ),
    )
    .unwrap();
    executable
}

#[test]
#[ignore = "requires logged-in macOS + Accessibility; task ui:smoke"]
fn native_menu_dialog_update_and_quit() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("generated");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("first.bmp"), support::bmp([1, 20, 200])).unwrap();
    let path = dir.path().join("native-smoke.sqlite");
    let mut context = Context::open(&path).unwrap();
    dispatch_guarded(&mut context, "init", &Params::new()).unwrap();
    let mut params = Params::new();
    params.set("path", media.to_string_lossy());
    dispatch_guarded(&mut context, "import", &params).unwrap();
    assert_eq!(context.catalog().stats().unwrap().assets, 1);
    std::fs::write(media.join("second.bmp"), support::bmp([200, 20, 1])).unwrap();
    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir).unwrap();
    std::fs::write(log_dir.join("native-automation.log"), "").unwrap();
    let stderr = std::fs::File::create(log_dir.join("native-stderr.log")).unwrap();
    let mut app = OwnedProcess(
        Command::new(isolated_executable(dir.path()))
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let pid = app.0.id();
    automate(
        pid,
        &format!(
            r#"
        repeat 200 times
            if (exists window 1) and (exists menu item "Update" of menu "File" of menu bar item "File" of menu bar 1) then exit repeat
            delay 0.05
        end repeat
        if not (exists window 1) then error "No {APP_NAME} window"
        if not (exists menu item "Update" of menu "File" of menu bar item "File" of menu bar 1) then error "No File → Update menu"
        log "Smoke window and File menu ready"
        repeat 200 times
            set appMenuName to name of menu bar item 2 of menu bar 1
            if appMenuName is "{APP_NAME}" then exit repeat
            delay 0.05
        end repeat
        if appMenuName is not "{APP_NAME}" then error "Expected {APP_NAME} app menu, got: " & appMenuName
        log "Application menu name verified"
        click menu bar item "File" of menu bar 1
        click menu item "Update" of menu "File" of menu bar item "File" of menu bar 1
        log "Update clicked"
    "#
        ),
    );
    wait_for("native Update to import the second generated image", || {
        assert!(
            app.0.try_wait().unwrap().is_none(),
            "{APP_NAME} exited; see target/ui-smoke/native-stderr.log"
        );
        context.catalog().stats().unwrap().assets == 2
    });
    automate(
        pid,
        r#"
        repeat 200 times
            if enabled of menu item "Import…" of menu "File" of menu bar item "File" of menu bar 1 then exit repeat
            delay 0.05
        end repeat
        click menu bar item "File" of menu bar 1
        click menu item "Import…" of menu "File" of menu bar item "File" of menu bar 1
        log "Import clicked"
        repeat 200 times
            if (exists sheet 1 of window 1) or (count of windows > 1) then exit repeat
            delay 0.05
        end repeat
        if not ((exists sheet 1 of window 1) or (count of windows > 1)) then error "Import did not open a native picker"
        log "Picker opened"
        repeat 200 times
            if not (exists sheet 1 of window 1) and (count of windows is 1) then exit repeat
            key code 53
            delay 0.05
        end repeat
        if (exists sheet 1 of window 1) or (count of windows > 1) then error "Escape did not dismiss the picker"
        log "Picker dismissed"
        keystroke "q" using command down
    "#,
    );
    wait_for(&format!("Command-Q to close {APP_NAME}"), || {
        app.0.try_wait().unwrap().is_some()
    });
    assert!(app.0.wait().unwrap().success());
    let reopened = Context::open(&path).unwrap();
    assert_eq!(
        reopened.catalog().stats().unwrap().assets,
        2,
        "cancelling Import must not change the catalogue"
    );
}
