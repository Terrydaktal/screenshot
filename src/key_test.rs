fn main() {
    let mut devices = evdev::enumerate().collect::<Vec<_>>();
    devices.sort_by_key(|(p, _)| p.clone());

    for (path, mut device) in devices {
        if device.name().map_or(false, |n| n.contains("Keyboard") || n.contains("STRAFE")) {
            println!("Testing device: {:?}", path);
            std::thread::spawn(move || {
                loop {
                    for event in device.fetch_events().expect("Error fetching events") {
                        if event.event_type() == evdev::EventType::KEY && event.value() == 1 {
                            println!("DEVICE {:?} | CODE: {} | NAME: {:?}", path, event.code(), event.code());
                        }
                    }
                }
            });
        }
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
