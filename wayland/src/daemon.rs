mod capture;

use anyhow::{Context, Result};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

const SERVICE_NAME: &str = "io.github.terrydaktal.Screenshot";
const OBJECT_PATH: &str = "/io/github/terrydaktal/Screenshot";
const SCREENSHOT_INTERFACE: &str = "io.github.terrydaktal.Screenshot";
const LAUNCH_COOLDOWN: Duration = Duration::from_millis(350);
const GUI_ENV_KEYS: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

#[derive(Clone)]
struct TriggerService {
    connection: Connection,
    screenshot_bin: PathBuf,
    state: Arc<Mutex<LaunchState>>,
}

struct LaunchState {
    busy: bool,
    last_trigger: Instant,
}

#[zbus::interface(name = "io.github.terrydaktal.Screenshot")]
impl TriggerService {
    fn trigger(&self) -> bool {
        {
            let mut state = self.state.lock().expect("launch state mutex poisoned");
            if state.busy || state.last_trigger.elapsed() < LAUNCH_COOLDOWN {
                return false;
            }
            state.busy = true;
            state.last_trigger = Instant::now();
        }

        let connection = self.connection.clone();
        let screenshot_bin = self.screenshot_bin.clone();
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            if let Err(err) = capture_and_run(&connection, &screenshot_bin) {
                eprintln!("ERROR: screenshot trigger failed: {err:#}");
            }
            let mut state = state.lock().expect("launch state mutex poisoned");
            state.busy = false;
            state.last_trigger = Instant::now();
        });

        true
    }
}

fn main() -> Result<()> {
    if env::args().nth(1).as_deref() == Some("--capture-test") {
        return run_capture_test();
    }

    let screenshot_bin = resolve_screenshot_bin()?;
    let connection = Connection::session().context("failed to connect to the session D-Bus")?;
    connection
        .request_name(SERVICE_NAME)
        .with_context(|| format!("failed to own D-Bus service {SERVICE_NAME}"))?;

    let service = TriggerService {
        connection: connection.clone(),
        screenshot_bin,
        state: Arc::new(Mutex::new(LaunchState {
            busy: false,
            last_trigger: Instant::now() - Duration::from_secs(10),
        })),
    };
    connection
        .object_server()
        .at(OBJECT_PATH, service)
        .with_context(|| format!("failed to serve {SCREENSHOT_INTERFACE} at {OBJECT_PATH}"))?;

    println!("READY: Wayland screenshot daemon listening on D-Bus service {SERVICE_NAME}");
    loop {
        std::thread::park();
    }
}

fn run_capture_test() -> Result<()> {
    let connection = Connection::session().context("failed to connect to the session D-Bus")?;
    let started = Instant::now();
    let frame = capture::capture_workspace(&connection)?;
    println!(
        "Captured {}x{} workspace at {:.2}x in {:.2} ms ({} RGBA bytes)",
        frame.width,
        frame.height,
        frame.scale,
        started.elapsed().as_secs_f64() * 1000.0,
        frame.rgba.len()
    );
    Ok(())
}

fn capture_and_run(connection: &Connection, screenshot_bin: &Path) -> Result<()> {
    let started = Instant::now();
    let frame = capture::capture_workspace(connection)?;
    let capture_elapsed = started.elapsed();

    let mut command = Command::new(screenshot_bin);
    command
        .arg("--stdin-rgba")
        .arg(frame.width.to_string())
        .arg(frame.height.to_string())
        .arg(frame.scale.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    apply_current_gui_environment(&mut command);

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to launch {}", screenshot_bin.display()))?;
    let mut stdin = child
        .stdin
        .take()
        .context("screenshot overlay stdin was not piped")?;
    stdin
        .write_all(&frame.rgba)
        .context("failed to pass captured frame to screenshot overlay")?;
    drop(stdin);

    eprintln!(
        "INFO: captured {}x{} workspace at {:.2}x in {:.2} ms",
        frame.width,
        frame.height,
        frame.scale,
        capture_elapsed.as_secs_f64() * 1000.0
    );
    let status = child
        .wait()
        .context("failed to wait for screenshot overlay")?;
    if !status.success() {
        anyhow::bail!("screenshot overlay exited with {status}");
    }
    Ok(())
}

fn resolve_screenshot_bin() -> Result<PathBuf> {
    let daemon_path = env::current_exe().context("failed to resolve daemon executable path")?;
    let bin_dir = daemon_path
        .parent()
        .context("daemon executable has no parent directory")?;
    let screenshot_bin = bin_dir.join("screenshot");
    if !screenshot_bin.is_file() {
        anyhow::bail!(
            "screenshot binary not found beside daemon at {}",
            screenshot_bin.display()
        );
    }
    Ok(screenshot_bin)
}

fn apply_current_gui_environment(command: &mut Command) {
    let Ok(output) = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if GUI_ENV_KEYS.contains(&key) {
            command.env(key, value);
        }
    }
}
