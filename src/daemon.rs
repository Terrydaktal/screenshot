use captrs::{Bgr8, CaptureError, Capturer};
use evdev::{Device, EventType, KeyCode};
use std::collections::HashSet;
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SCREENSHOT_BIN: &str = "/home/lewis/Dev/screenshot/target/release/screenshot";
const LAUNCH_COOLDOWN_MS: u64 = 1000;
const CAPTURE_RETRIES: usize = 6;
const CAPTURE_RETRY_SLEEP_MS: u64 = 2;
const DEVICE_SCAN_INTERVAL_MS: u64 = 1500;
const INPUT_DIR: &str = "/dev/input";
const KEY_PRINTSCREEN_SYSRQ: u16 = 99;
const KEY_SCROLL_LOCK: u16 = 70;
const KEY_PAUSE: u16 = 119;
const KEY_PRINTSCREEN: u16 = 210;
const GUI_ENV_KEYS: &[&str] = &[
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

fn main() {
    let screenshot_bin = resolve_screenshot_bin();
    let last_launch = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));
    let active_listeners = Arc::new(Mutex::new(HashSet::<String>::new()));
    let mut warned_no_listeners = false;

    loop {
        attach_input_listeners(&screenshot_bin, &last_launch, &active_listeners);

        let active_count = active_listeners.lock().unwrap().len();
        if active_count == 0 {
            if !warned_no_listeners {
                eprintln!(
                    "WARN: no keyboard listeners active; retrying device scan every {} ms",
                    DEVICE_SCAN_INTERVAL_MS
                );
                warned_no_listeners = true;
            }
        } else {
            warned_no_listeners = false;
        }

        std::thread::sleep(Duration::from_millis(DEVICE_SCAN_INTERVAL_MS));
    }
}

fn attach_input_listeners(
    screenshot_bin: &str,
    last_launch: &Arc<Mutex<Instant>>,
    active_listeners: &Arc<Mutex<HashSet<String>>>,
) {
    let Ok(entries) = std::fs::read_dir(INPUT_DIR) else {
        eprintln!("WARN: failed to read {INPUT_DIR}; will retry");
        return;
    };

    for entry in entries.flatten() {
        let path_buf = entry.path();
        let Some(file_name) = path_buf.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("event") {
            continue;
        }

        let path = path_buf.to_string_lossy().to_string();
        {
            let mut active = active_listeners.lock().unwrap();
            if active.contains(&path) {
                continue;
            }
            active.insert(path.clone());
        }

        match Device::open(&path) {
            Ok(device) => {
                if !supports_trigger_key(&device) {
                    active_listeners.lock().unwrap().remove(&path);
                    continue;
                }

                let name = device.name().unwrap_or_default().to_lowercase();
                println!("READY: Listening on {} ({})", path, name);
                let last_launch_clone = Arc::clone(last_launch);
                let active_clone = Arc::clone(active_listeners);
                let screenshot_bin_clone = screenshot_bin.to_string();

                std::thread::spawn(move || {
                    run_input_listener(path, screenshot_bin_clone, last_launch_clone, active_clone);
                });
            }
            Err(err) => {
                active_listeners.lock().unwrap().remove(&path);
                eprintln!("WARN: failed to open {path}: {err}; will retry");
            }
        }
    }
}

fn run_input_listener(
    path: String,
    screenshot_bin: String,
    last_launch: Arc<Mutex<Instant>>,
    active_listeners: Arc<Mutex<HashSet<String>>>,
) {
    let result: Result<(), String> = (|| {
        let mut device = Device::open(&path).map_err(|err| format!("Failed to reopen: {err}"))?;
        loop {
            let events = device
                .fetch_events()
                .map_err(|err| format!("Read error: {err}"))?;
            for event in events {
                if event.event_type() != EventType::KEY || event.value() != 1 {
                    continue;
                }

                let code = event.code();
                // Support common PrintScreen codes plus fallback keys.
                if code != KEY_PRINTSCREEN_SYSRQ
                    && code != KEY_PRINTSCREEN
                    && code != KEY_SCROLL_LOCK
                    && code != KEY_PAUSE
                {
                    continue;
                }

                let mut should_launch = false;
                {
                    let mut last = last_launch.lock().unwrap();
                    if last.elapsed() > Duration::from_millis(LAUNCH_COOLDOWN_MS) {
                        *last = Instant::now();
                        should_launch = true;
                    }
                }

                if !should_launch {
                    continue;
                }

                refresh_gui_environment();
                let env_vars = env::vars().collect::<Vec<_>>();

                match capture_rgb_frame_fast() {
                    Ok((width, height, rgb_pixels)) => {
                        match spawn_prefetched(
                            &screenshot_bin,
                            &env_vars,
                            width,
                            height,
                            &rgb_pixels,
                        ) {
                            Ok(()) => continue,
                            Err(err) => {
                                eprintln!(
                                    "WARN: prefetched screenshot launch failed ({err}); falling back to plain launch"
                                );
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("WARN: pre-capture failed ({err}); falling back to plain launch");
                    }
                }

                if let Err(err) = spawn_plain(&screenshot_bin, &env_vars) {
                    eprintln!("ERROR: plain screenshot launch failed: {err}");
                }
            }
        }
    })();

    if let Err(err) = result {
        eprintln!("WARN: listener on {path} stopped: {err}");
    }

    active_listeners.lock().unwrap().remove(&path);
}

fn supports_trigger_key(device: &Device) -> bool {
    device.supported_keys().is_some_and(|keys| {
        keys.contains(KeyCode::new(KEY_PRINTSCREEN_SYSRQ))
            || keys.contains(KeyCode::new(KEY_PRINTSCREEN))
            || keys.contains(KeyCode::new(KEY_SCROLL_LOCK))
            || keys.contains(KeyCode::new(KEY_PAUSE))
    })
}

fn capture_rgb_frame_fast() -> Result<(usize, usize, Vec<u8>), String> {
    let mut capturer = Capturer::new(0).map_err(|err| format!("capturer init failed: {err}"))?;
    let (width, height) = capturer.geometry();
    let image_data = capture_frame_with_retries(&mut capturer)
        .map_err(|err| format!("capture_frame failed: {err:?}"))?;

    let mut rgb_pixels = Vec::with_capacity((width * height * 3) as usize);
    for pixel in image_data {
        rgb_pixels.push(pixel.r);
        rgb_pixels.push(pixel.g);
        rgb_pixels.push(pixel.b);
    }

    Ok((width as usize, height as usize, rgb_pixels))
}

fn capture_frame_with_retries(capturer: &mut Capturer) -> Result<Vec<Bgr8>, CaptureError> {
    let mut last_error: Option<CaptureError> = None;

    for attempt in 0..=CAPTURE_RETRIES {
        match capturer.capture_frame() {
            Ok(frame) => return Ok(frame),
            Err(err) => {
                last_error = Some(err);
                if attempt < CAPTURE_RETRIES {
                    std::thread::sleep(Duration::from_millis(CAPTURE_RETRY_SLEEP_MS));
                }
            }
        }
    }

    Err(last_error.expect("capture_frame_with_retries exhausted without capture attempt"))
}

fn spawn_prefetched(
    screenshot_bin: &str,
    env_vars: &[(String, String)],
    width: usize,
    height: usize,
    rgb_pixels: &[u8],
) -> std::io::Result<()> {
    let mut cmd = Command::new(screenshot_bin);
    cmd.arg("--stdin-rgb")
        .arg(width.to_string())
        .arg(height.to_string());
    configure_child_env(&mut cmd, env_vars);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(rgb_pixels)?;
    }

    Ok(())
}

fn spawn_plain(screenshot_bin: &str, env_vars: &[(String, String)]) -> std::io::Result<()> {
    let mut cmd = Command::new(screenshot_bin);
    configure_child_env(&mut cmd, env_vars);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::inherit());
    let _ = cmd.spawn()?;
    Ok(())
}

fn configure_child_env(cmd: &mut Command, env_vars: &[(String, String)]) {
    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    if env::var_os("RUST_BACKTRACE").is_none() {
        cmd.env("RUST_BACKTRACE", "full");
    }
    if env::var_os("RUST_LIB_BACKTRACE").is_none() {
        cmd.env("RUST_LIB_BACKTRACE", "full");
    }
}

fn refresh_gui_environment() {
    let Ok(output) = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
    else {
        eprintln!("WARN: failed to query user systemd environment");
        return;
    };

    if !output.status.success() {
        eprintln!(
            "WARN: systemctl --user show-environment failed with status {}",
            output.status
        );
        return;
    }

    let env_text = String::from_utf8_lossy(&output.stdout);
    for line in env_text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if GUI_ENV_KEYS.contains(&key) {
            env::set_var(key, value);
        }
    }
}

fn resolve_screenshot_bin() -> String {
    let fallback = SCREENSHOT_BIN.to_string();
    let Ok(exe_path) = env::current_exe() else {
        eprintln!("WARN: failed to resolve daemon executable path; using fallback screenshot path");
        return fallback;
    };

    let Some(exe_dir) = exe_path.parent() else {
        eprintln!(
            "WARN: daemon executable has no parent directory; using fallback screenshot path"
        );
        return fallback;
    };

    let candidate = exe_dir.join("screenshot");
    if !candidate.exists() {
        eprintln!(
            "WARN: screenshot binary not found at {}; using fallback path",
            candidate.display()
        );
        return fallback;
    }

    candidate.to_string_lossy().to_string()
}
