use captrs::{Bgr8, CaptureError, Capturer};
use evdev::{Device, EventType};
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SCREENSHOT_BIN: &str = "/home/lewis/Dev/screenshot/target/release/screenshot";
const LAUNCH_COOLDOWN_MS: u64 = 1000;
const CAPTURE_RETRIES: usize = 6;
const CAPTURE_RETRY_SLEEP_MS: u64 = 2;

fn main() {
    let env_vars: Vec<(String, String)> = env::vars().collect();
    let last_launch = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));

    for i in 0..20 {
        let path = format!("/dev/input/event{}", i);
        if let Ok(d) = Device::open(&path) {
            let name = d.name().unwrap_or_default().to_lowercase();
            if name.contains("keyboard") || name.contains("strafe") {
                println!("READY: Listening on {} ({})", path, name);

                let last_launch_clone = last_launch.clone();
                let env_vars_clone = env_vars.clone();

                std::thread::spawn(move || {
                    let mut device = Device::open(&path).expect("Failed to reopen");
                    loop {
                        for event in device.fetch_events().expect("Read error") {
                            if event.event_type() != EventType::KEY || event.value() != 1 {
                                continue;
                            }

                            let code = event.code();
                            // Support PrintScreen (99), ScrollLock (70), or Pause (119)
                            if code != 99 && code != 70 && code != 119 {
                                continue;
                            }

                            let mut should_launch = false;
                            {
                                let mut last = last_launch_clone.lock().unwrap();
                                if last.elapsed() > Duration::from_millis(LAUNCH_COOLDOWN_MS) {
                                    *last = Instant::now();
                                    should_launch = true;
                                }
                            }

                            if !should_launch {
                                continue;
                            }

                            if let Ok((width, height, rgb_pixels)) = capture_rgb_frame_fast() {
                                if spawn_prefetched(&env_vars_clone, width, height, &rgb_pixels)
                                    .is_ok()
                                {
                                    continue;
                                }
                            }

                            let _ = spawn_plain(&env_vars_clone);
                        }
                    }
                });
            }
        }
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
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
    env_vars: &[(String, String)],
    width: usize,
    height: usize,
    rgb_pixels: &[u8],
) -> std::io::Result<()> {
    let mut cmd = Command::new(SCREENSHOT_BIN);
    cmd.arg("--stdin-rgb")
        .arg(width.to_string())
        .arg(height.to_string());
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(rgb_pixels)?;
    }

    Ok(())
}

fn spawn_plain(env_vars: &[(String, String)]) -> std::io::Result<()> {
    let mut cmd = Command::new(SCREENSHOT_BIN);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    let _ = cmd.spawn()?;
    Ok(())
}
