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
<key>CFBundleDocumentTypes</key><array><dict>
  <key>CFBundleTypeName</key><string>iCatalog Catalogue</string>
  <key>CFBundleTypeRole</key><string>Editor</string>
  <key>LSItemContentTypes</key><array><string>org.icatalog.catalogue</string></array>
</dict></array>
<key>UTExportedTypeDeclarations</key><array><dict>
  <key>UTTypeIdentifier</key><string>org.icatalog.catalogue</string>
  <key>UTTypeConformsTo</key><array><string>public.data</string></array>
  <key>UTTypeTagSpecification</key><dict>
    <key>public.filename-extension</key><array><string>icatalog</string></array>
  </dict>
</dict></array>
</dict></plist>"#,
            std::process::id()
        ),
    )
    .unwrap();
    executable
}

struct FinderApp {
    pid: u32,
    executable: PathBuf,
}

impl Drop for FinderApp {
    fn drop(&mut self) {
        if process_id_for(&self.executable) == Some(self.pid) {
            let _ = Command::new("kill").arg(self.pid.to_string()).status();
        }
    }
}

fn process_id_for(executable: &Path) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,command="])
        .output()
        .ok()?;
    let needle = executable
        .canonicalize()
        .ok()?
        .to_string_lossy()
        .into_owned();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(' ')?;
            if command.trim_start().starts_with(&needle) {
                pid.parse::<u32>().ok()
            } else {
                None
            }
        })
        .next()
}

fn process_has_catalogue(pid: u32, catalogue: &Path) -> bool {
    let Ok(output) = Command::new("lsof")
        .args(["-nP", "-p", &pid.to_string(), "-Fn"])
        .output()
    else {
        return false;
    };
    let Ok(canonical) = catalogue.canonicalize() else {
        return false;
    };
    let target = format!("n{}", canonical.display());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line == target)
}

#[test]
#[ignore = "requires logged-in macOS LaunchServices; task ui:smoke"]
fn finder_opens_catalogues_on_cold_and_running_app() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.icatalog");
    let second = dir.path().join("second catalogue.icatalog");
    for path in [&first, &second] {
        let mut ctx = Context::open(path).unwrap();
        dispatch_guarded(&mut ctx, "init", &Params::new()).unwrap();
    }
    let executable = isolated_executable(dir.path());
    let bundle = dir.path().join("iCatalog.app");
    let opened = Command::new("open")
        .arg("-n")
        .arg("-a")
        .arg(&bundle)
        .arg(&first)
        .output()
        .unwrap();
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    let mut pid = None;
    wait_for("Finder cold launch to open the first catalogue", || {
        pid = process_id_for(&executable);
        pid.is_some_and(|id| process_has_catalogue(id, &first))
    });
    let app = FinderApp {
        pid: pid.unwrap(),
        executable,
    };
    let opened = Command::new("open")
        .arg("-a")
        .arg(&bundle)
        .arg(&second)
        .output()
        .unwrap();
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    wait_for("Finder running-app event to replace the catalogue", || {
        process_has_catalogue(app.pid, &second) && !process_has_catalogue(app.pid, &first)
    });
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
        if not (exists menu item "Quit {APP_NAME}" of menu 1 of menu bar item 2 of menu bar 1) then error "No routed Quit menu item"
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
        if not (exists menu item "Connect to Server…" of menu "File" of menu bar item "File" of menu bar 1) then error "No File → Connect to Server menu"
        if not (exists menu item "Sync…" of menu "File" of menu bar item "File" of menu bar 1) then error "No File → Sync menu"
        click menu bar item "File" of menu bar 1
        click menu item "Connect to Server…" of menu "File" of menu bar item "File" of menu bar 1
        repeat 200 times
            if not (enabled of menu item "Sync…" of menu "File" of menu bar item "File" of menu bar 1) then exit repeat
            delay 0.05
        end repeat
        if enabled of menu item "Sync…" of menu "File" of menu bar item "File" of menu bar 1 then error "Connect did not open its modal"
        key code 53
        repeat 200 times
            if enabled of menu item "Sync…" of menu "File" of menu bar item "File" of menu bar 1 then exit repeat
            delay 0.05
        end repeat
        if not (enabled of menu item "Sync…" of menu "File" of menu bar item "File" of menu bar 1) then error "Escape did not dismiss Connect"
        click menu bar item "File" of menu bar 1
        click menu item "Sync…" of menu "File" of menu bar item "File" of menu bar 1
        repeat 200 times
            if not (enabled of menu item "Connect to Server…" of menu "File" of menu bar item "File" of menu bar 1) then exit repeat
            delay 0.05
        end repeat
        if enabled of menu item "Connect to Server…" of menu "File" of menu bar item "File" of menu bar 1 then error "Sync did not open its modal"
        key code 53
        repeat 200 times
            if enabled of menu item "Connect to Server…" of menu "File" of menu bar item "File" of menu bar 1 then exit repeat
            delay 0.05
        end repeat
        if not (enabled of menu item "Connect to Server…" of menu "File" of menu bar item "File" of menu bar 1) then error "Escape did not dismiss Sync"
        log "Native Connect and Sync menus verified without transfers"
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
