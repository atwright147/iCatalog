//! Runs only on an explicitly selected, logged-in macOS desktop.
//! System Events is the OS automation interface; the test driver is Rust.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod support;
use icatalog_engine::{dispatch_guarded, Context, Params};
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
            "Timed out waiting for {description}"
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
            set targetProcess to first application process whose unix id is {pid}
            tell targetProcess
                set frontmost to true
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
    wait_for(
        "macOS UI automation (enable Accessibility/Automation for the runner)",
        || child.0.try_wait().unwrap().is_some(),
    );
    let status = child.0.wait().unwrap();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.0.stderr.take() {
        use std::io::Read;
        pipe.read_to_string(&mut stderr).unwrap();
    }
    assert!(status.success(), "macOS automation failed: {stderr}");
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
    std::fs::write(media.join("second.bmp"), support::bmp([200, 20, 1])).unwrap();
    let log_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-smoke");
    std::fs::create_dir_all(&log_dir).unwrap();
    let stderr = std::fs::File::create(log_dir.join("native-stderr.log")).unwrap();
    let mut app = OwnedProcess(
        Command::new(env!("CARGO_BIN_EXE_icatalog-ui"))
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let pid = app.0.id();
    automate(
        pid,
        r#"
        repeat 200 times
            if exists window 1 then exit repeat
            delay 0.05
        end repeat
        if not (exists window 1) then error "No iCatalog window"
        click menu item "Update" of menu "File" of menu bar item "File" of menu bar 1
    "#,
    );
    wait_for("native Update to import the second generated image", || {
        assert!(
            app.0.try_wait().unwrap().is_none(),
            "iCatalog exited; see target/ui-smoke/native-stderr.log"
        );
        context
            .catalog()
            .stats()
            .map(|stats| stats.assets == 2)
            .unwrap_or(false)
    });
    automate(
        pid,
        r#"
        repeat 200 times
            if enabled of menu item "Import…" of menu "File" of menu bar item "File" of menu bar 1 then exit repeat
            delay 0.05
        end repeat
        click menu item "Import…" of menu "File" of menu bar item "File" of menu bar 1
        repeat 200 times
            if (exists sheet 1 of window 1) or (count of windows > 1) then exit repeat
            delay 0.05
        end repeat
        if not ((exists sheet 1 of window 1) or (count of windows > 1)) then error "Import did not open a native picker"
        key code 53
        repeat 200 times
            if not (exists sheet 1 of window 1) and (count of windows is 1) then exit repeat
            delay 0.05
        end repeat
        if (exists sheet 1 of window 1) or (count of windows > 1) then error "Escape did not dismiss the picker"
        keystroke "q" using command down
    "#,
    );
    wait_for("Command-Q to close iCatalog", || {
        app.0.try_wait().unwrap().is_some()
    });
    assert!(app.0.wait().unwrap().success());
    assert_eq!(
        context.catalog().stats().unwrap().assets,
        2,
        "cancelling Import must not change the catalogue"
    );
}
