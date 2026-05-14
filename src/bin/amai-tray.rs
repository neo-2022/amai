use ksni::menu::{MenuItem, StandardItem};
use ksni::Tray;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct AmaiTray {
    repo_root: PathBuf,
}

impl AmaiTray {
    fn run_script(&self, rel: &str, args: &[&str]) {
        let script = self.repo_root.join(rel);
        let _ = Command::new(script).args(args).status();
    }

    fn spawn_script(&self, rel: &str, args: &[&str]) {
        let script = self.repo_root.join(rel);
        let _ = Command::new(script).args(args).spawn();
    }

    fn status_label(&self) -> String {
        let script = self.repo_root.join("scripts/amai_tray_menu.sh");
        let output = Command::new(script).arg("--status").output();
        if let Ok(result) = output {
            let raw = String::from_utf8_lossy(&result.stdout);
            let text = raw.trim();
            if !text.is_empty() {
                return format!("Статус: {text}");
            }
        }
        "Статус: неизвестно".to_string()
    }

    fn notifications_label(&self) -> String {
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let mut base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                base.push(".config");
                base
            });
        let disabled = config_home.join("amai/tray_notifications_disabled");
        if disabled.exists() {
            "Включить уведомления".to_string()
        } else {
            "Не показывать уведомления".to_string()
        }
    }

    fn notify(&self, text: &str) {
        let _ = Command::new("notify-send").arg("Amai").arg(text).status();
    }
}

impl Tray for AmaiTray {
    fn id(&self) -> String {
        "amai".to_string()
    }

    fn title(&self) -> String {
        "Amai".to_string()
    }

    fn icon_name(&self) -> String {
        "amai".to_string()
    }

    fn icon_theme_path(&self) -> String {
        if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
            return format!("{xdg_data_home}/icons");
        }
        if let Some(home) = dirs::home_dir() {
            return home.join(".local/share/icons").display().to_string();
        }
        String::new()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let status = self.status_label();
        let notifications_label = self.notifications_label();

        vec![
            StandardItem {
                label: status,
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Открыть панель Amai".into(),
                activate: Box::new(|tray: &mut AmaiTray| {
                    tray.spawn_script("scripts/amai_tray_menu.sh", &["--menu"]);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Подключить к VS Code/Codium".into(),
                activate: Box::new(|tray: &mut AmaiTray| {
                    tray.run_script(
                        "scripts/install_amai.sh",
                        &["--client", "vscode", "--stack-profile", "default", "--yes"],
                    );
                    tray.notify("Amai подключена к VS Code/Codium.");
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Проверить подключение".into(),
                activate: Box::new(|tray: &mut AmaiTray| {
                    tray.run_script("scripts/amai_tray_menu.sh", &["--check"]);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Исправить автоматически".into(),
                activate: Box::new(|tray: &mut AmaiTray| {
                    tray.run_script("scripts/amai_tray_menu.sh", &["--repair"]);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: notifications_label,
                activate: Box::new(|tray: &mut AmaiTray| {
                    tray.run_script("scripts/amai_tray_menu.sh", &["--toggle-notifications"]);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Удалить Amai полностью".into(),
                activate: Box::new(|tray: &mut AmaiTray| {
                    tray.run_script("scripts/amai_tray_menu.sh", &["--remove-full"]);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Выход".into(),
                activate: Box::new(|_tray: &mut AmaiTray| {
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn main() {
    let repo_root = std::env::var("AMAI_REPO_ROOT")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                exe.parent()
                    .and_then(|dir| dir.parent())
                    .map(PathBuf::from)
            })
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let tray = AmaiTray { repo_root };
    let service = ksni::TrayService::new(tray);
    service.spawn();
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
