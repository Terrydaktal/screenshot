use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use zbus::blocking::{Connection, Proxy};

const PRINT_KEY: i32 = 0x0100_0009;
const SCREENSHOT_COMPONENT: &str = "kwin";
const SCREENSHOT_ACTION: &str = "ScreenshotWaylandCapture";

type ShortcutInfo = (
    String,
    String,
    String,
    String,
    String,
    String,
    Vec<i32>,
    Vec<i32>,
);

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ShortcutBackup {
    version: u32,
    shortcuts: Vec<ShortcutRecord>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ShortcutRecord {
    action: String,
    action_name: String,
    component: String,
    component_name: String,
    keys: Vec<i32>,
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next();
    let path = args.next().map(PathBuf::from);
    if args.next().is_some() {
        bail!(usage());
    }

    match (mode.as_deref().and_then(|value| value.to_str()), path) {
        (Some("--install"), Some(path)) => install_shortcut(&path),
        (Some("--restore"), Some(path)) => restore_shortcuts(&path),
        (Some("--release"), None) => release_shortcut(),
        _ => bail!(usage()),
    }
}

fn install_shortcut(backup_path: &Path) -> Result<()> {
    let connection = Connection::session().context("failed to connect to the session D-Bus")?;
    let proxy = shortcut_proxy(&connection)?;

    let conflicts = shortcuts_for_key(&proxy, PRINT_KEY)?;
    let backup = ShortcutBackup {
        version: 1,
        shortcuts: conflicts
            .iter()
            .filter(|info| info.2 != SCREENSHOT_COMPONENT || info.0 != SCREENSHOT_ACTION)
            .map(shortcut_record)
            .collect(),
    };
    write_backup_once(backup_path, &backup)?;

    for (action, action_name, component, component_name, _, _, keys, _) in conflicts {
        if component == SCREENSHOT_COMPONENT && action == SCREENSHOT_ACTION {
            continue;
        }

        let remaining_keys: Vec<i32> = keys.into_iter().filter(|key| *key != PRINT_KEY).collect();
        set_shortcut(
            &proxy,
            vec![
                component.clone(),
                action.clone(),
                component_name,
                action_name,
            ],
            remaining_keys,
        )
        .with_context(|| format!("failed to release Print from {component}/{action}"))?;
        println!("Released Print from {component}/{action}");
    }

    set_shortcut(&proxy, screenshot_action_id(), vec![PRINT_KEY])
        .context("failed to assign Print to screenshot")?;

    let owners = shortcuts_for_key(&proxy, PRINT_KEY)?;
    let assigned = owners.iter().any(|info| {
        info.2 == SCREENSHOT_COMPONENT && info.0 == SCREENSHOT_ACTION && info.6.contains(&PRINT_KEY)
    });
    if !assigned {
        anyhow::bail!("KGlobalAccel did not assign Print to screenshot");
    }

    println!("Assigned Print to {SCREENSHOT_COMPONENT}/{SCREENSHOT_ACTION}");
    Ok(())
}

fn restore_shortcuts(backup_path: &Path) -> Result<()> {
    let backup = read_backup(backup_path)?;

    let connection = Connection::session().context("failed to connect to the session D-Bus")?;
    let proxy = shortcut_proxy(&connection)?;
    release_screenshot_shortcut(&proxy)?;

    for record in &backup.shortcuts {
        set_shortcut(
            &proxy,
            vec![
                record.component.clone(),
                record.action.clone(),
                record.component_name.clone(),
                record.action_name.clone(),
            ],
            record.keys.clone(),
        )
        .with_context(|| {
            format!(
                "failed to restore shortcuts for {}/{}",
                record.component, record.action
            )
        })?;
    }

    for record in &backup.shortcuts {
        for key in &record.keys {
            let owners = shortcuts_for_key(&proxy, *key)?;
            if !owners
                .iter()
                .any(|info| info.2 == record.component && info.0 == record.action)
            {
                bail!(
                    "KGlobalAccel did not restore key {key} to {}/{}",
                    record.component,
                    record.action
                );
            }
        }
        println!(
            "Restored shortcuts for {}/{}",
            record.component, record.action
        );
    }

    Ok(())
}

fn release_shortcut() -> Result<()> {
    let connection = Connection::session().context("failed to connect to the session D-Bus")?;
    let proxy = shortcut_proxy(&connection)?;
    release_screenshot_shortcut(&proxy)
}

fn release_screenshot_shortcut(proxy: &Proxy<'_>) -> Result<()> {
    for (action, _, component, _, _, _, keys, _) in shortcuts_for_key(proxy, PRINT_KEY)? {
        if component != SCREENSHOT_COMPONENT || action != SCREENSHOT_ACTION {
            continue;
        }
        let remaining_keys = keys.into_iter().filter(|key| *key != PRINT_KEY).collect();
        set_shortcut(proxy, screenshot_action_id(), remaining_keys)
            .context("failed to release Print from screenshot")?;
        println!("Released Print from {SCREENSHOT_COMPONENT}/{SCREENSHOT_ACTION}");
    }
    Ok(())
}

fn shortcut_proxy(connection: &Connection) -> Result<Proxy<'_>> {
    Proxy::new(
        connection,
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel",
    )
    .context("failed to create KGlobalAccel proxy")
}

fn shortcuts_for_key(proxy: &Proxy<'_>, key: i32) -> Result<Vec<ShortcutInfo>> {
    proxy
        .call("getGlobalShortcutsByKey", &key)
        .with_context(|| format!("failed to query global shortcuts for key {key}"))
}

fn set_shortcut(proxy: &Proxy<'_>, action_id: Vec<String>, keys: Vec<i32>) -> Result<()> {
    proxy
        .call("setForeignShortcut", &(action_id, keys))
        .context("KGlobalAccel setForeignShortcut failed")
}

fn shortcut_record(info: &ShortcutInfo) -> ShortcutRecord {
    ShortcutRecord {
        action: info.0.clone(),
        action_name: info.1.clone(),
        component: info.2.clone(),
        component_name: info.3.clone(),
        keys: info.6.clone(),
    }
}

fn screenshot_action_id() -> Vec<String> {
    vec![
        SCREENSHOT_COMPONENT.to_string(),
        SCREENSHOT_ACTION.to_string(),
        "KWin".to_string(),
        "Capture with screenshot".to_string(),
    ]
}

fn write_backup_once(path: &Path, backup: &ShortcutBackup) -> Result<()> {
    if path.exists() {
        read_backup(path)?;
        println!("Keeping existing shortcut backup: {}", path.display());
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(backup).context("failed to encode shortcut backup")?;
    let temporary_path = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary_path, bytes).with_context(|| {
        format!(
            "failed to write temporary shortcut backup {}",
            temporary_path.display()
        )
    })?;
    fs::rename(&temporary_path, path)
        .with_context(|| format!("failed to save shortcut backup {}", path.display()))?;
    println!("Saved existing Print shortcuts to {}", path.display());
    Ok(())
}

fn read_backup(path: &Path) -> Result<ShortcutBackup> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read shortcut backup {}", path.display()))?;
    let backup: ShortcutBackup = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid shortcut backup {}", path.display()))?;
    if backup.version != 1 {
        bail!("unsupported shortcut backup version {}", backup.version);
    }
    Ok(backup)
}

fn usage() -> &'static str {
    "usage: screenshot-shortcut-setup --install BACKUP | --restore BACKUP | --release"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_shortcut_backup_is_never_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("screenshot-shortcut-test-{}", std::process::id()));
        let path = directory.join("shortcuts.json");
        let original = ShortcutBackup {
            version: 1,
            shortcuts: vec![ShortcutRecord {
                action: "original-action".to_string(),
                action_name: "Original action".to_string(),
                component: "original-component".to_string(),
                component_name: "Original component".to_string(),
                keys: vec![PRINT_KEY, 42],
            }],
        };
        let replacement = ShortcutBackup {
            version: 1,
            shortcuts: Vec::new(),
        };

        write_backup_once(&path, &original).expect("first backup write should succeed");
        write_backup_once(&path, &replacement).expect("existing backup should remain valid");
        assert_eq!(
            read_backup(&path).expect("saved backup should decode"),
            original
        );

        fs::remove_dir_all(directory).expect("test backup directory should be removable");
    }
}
