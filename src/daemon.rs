use evdev::Device;
use std::env;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    println!("READY: Screenshot Daemon Starting...");

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
                            if event.value() == 1 {
                                let code = event.code();
                                // Support PrintScreen (99), ScrollLock (70), or Pause (119)
                                if code == 99 || code == 70 || code == 119 {
                                    let mut last = last_launch_clone.lock().unwrap();
                                    if last.elapsed() > Duration::from_millis(1000) {
                                        println!("MATCH: Code {} on {}. Launching...", code, path);

                                        let mut cmd = Command::new(
                                            "/home/lewis/Dev/screenshot/target/release/screenshot",
                                        );
                                        for (k, v) in &env_vars_clone {
                                            cmd.env(k, v);
                                        }
                                        cmd.stdout(Stdio::inherit());
                                        cmd.stderr(Stdio::inherit());
                                        let _ = cmd.spawn();

                                        *last = Instant::now();
                                    }
                                }
                            }
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
