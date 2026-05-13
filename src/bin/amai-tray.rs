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

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let repo = self.repo_root.clone();
        vec![
            StandardItem {
                label: "Открыть меню Amai".into(),
                activate: Box::new(move |_tray: &mut AmaiTray| {
                    let _ = Command::new(repo.join("scripts/amai_tray_menu.sh"))
                        .arg("--menu")
                        .status();
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
                label: "Не показывать уведомления".into(),
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
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let tray = AmaiTray { repo_root };
    let service = ksni::TrayService::new(tray);
    service.spawn();
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}
