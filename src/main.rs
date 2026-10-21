use chrono::Local;
use eframe::egui::{self, Color32, Margin, Rounding, Stroke};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub game_dir: PathBuf,
    pub selected_server: String,
    pub servers: Vec<ServerInfo>,
    pub lang: String,
    pub full_check: bool,
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    #[serde(default, skip_serializing)]
    pub auto_check: Option<bool>,
    #[serde(default, skip_serializing)]
    pub auto_download: Option<bool>,
}

fn default_auto_update() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        let default_dir = dirs_default_game_dir();

        let default_servers = vec![
            ServerInfo {
                name: "Сервер 2 (Рекомендуется)".to_string(),
                url: "https://s2.hiddenfromhid.site/updater/".to_string(),
            },
            ServerInfo {
                name: "Сервер 1 (Основной)".to_string(),
                url: "https://hiddenfromhid.site/updater/".to_string(),
            },
            ServerInfo {
                name: "Сервер 4".to_string(),
                url: "https://s4.hiddenfromhid.site/updater/".to_string(),
            },
            ServerInfo {
                name: "Сервер 3".to_string(),
                url: "https://s3.hiddenfromhid.site/updater/".to_string(),
            },
        ];

        Self {
            game_dir: default_dir,
            selected_server: "https://s2.hiddenfromhid.site/updater/".to_string(),
            servers: default_servers,
            lang: "ru".to_string(),
            full_check: false,
            auto_update: true,
            auto_check: None,
            auto_download: None,
        }
    }
}

fn dirs_default_game_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            PathBuf::from("."),
            PathBuf::from(r"C:\Games\EndlessWar"),
            PathBuf::from(r"C:\EndlessWar"),
            PathBuf::from(r"D:\Games\EndlessWar"),
        ];
        for c in candidates {
            if c.join("system").join("EndlessWar.exe").exists() {
                return c;
            }
        }
        if let Ok(cur) = std::env::current_dir() {
            if cur.join("system").join("EndlessWar.exe").exists() {
                return cur;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(cur) = std::env::current_dir() {
            if cur.join("system").join("EndlessWar.exe").exists() {
                return cur;
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(home).join("PortProton/etc/EW");
            if p.exists() {
                return p;
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let p = PathBuf::from(appdata).join("open-ew-launcher");
                let _ = fs::create_dir_all(&p);
                return p.join("launcher.json");
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(home) = std::env::var("HOME") {
                let p = PathBuf::from(home).join(".config/open-ew-launcher");
                let _ = fs::create_dir_all(&p);
                return p.join("launcher.json");
            }
        }
        PathBuf::from("launcher.json")
    }

    pub fn open_config_folder() {
        let p = Self::config_path();
        if let Some(parent) = p.parent() {
            #[cfg(target_os = "windows")]
            {
                let _ = Command::new("explorer").arg(parent).spawn();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = Command::new("xdg-open").arg(parent).spawn();
            }
        }
    }

    pub fn load() -> Self {
        let p = Self::config_path();
        if let Ok(data) = fs::read_to_string(&p) {
            if let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&data) {
                if cfg.servers.is_empty() {
                    cfg.servers = Self::default().servers;
                }
                if let Some(ac) = cfg.auto_check {
                    cfg.auto_update = ac || cfg.auto_download.unwrap_or(false);
                }
                return cfg;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let p = Self::config_path();
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(p, data);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub filename: String,
    pub size: u64,
    pub original_crc: u32,
    pub archive_crc: u32,
    pub base_url: String,
}

pub fn parse_patchlist(xml_str: &str, base_url: &str) -> Vec<ArchiveEntry> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"archive" {
                    let mut size: u64 = 0;
                    let mut checksum = 0u32;
                    let mut original = 0u32;
                    let mut filename = String::new();

                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let val = String::from_utf8_lossy(&attr.value);
                        match key {
                            b"size" => size = val.parse().unwrap_or(0),
                            b"checksum" => {
                                checksum = u32::from_str_radix(val.trim_start_matches("0x"), 16).unwrap_or(0);
                            }
                            b"original" => {
                                original = u32::from_str_radix(val.trim_start_matches("0x"), 16).unwrap_or(0);
                            }
                            b"filename" => {
                                filename = val.replace('\\', "/");
                            }
                            _ => {}
                        }
                    }

                    if !filename.is_empty() {
                        entries.push(ArchiveEntry {
                            filename,
                            size,
                            original_crc: original,
                            archive_crc: checksum,
                            base_url: base_url.to_string(),
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    entries
}

pub fn ping_server_tcp(url_str: &str) -> (Option<u128>, String) {
    let parsed = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(e) => return (None, format!("Неверный URL: {}", e)),
    };

    let host = match parsed.host_str() {
        Some(h) => h,
        None => return (None, "Нет хоста в URL".to_string()),
    };

    let port = parsed.port().unwrap_or_else(|| {
        if parsed.scheme() == "https" { 443 } else { 80 }
    });

    let addr_str = format!("{}:{}", host, port);
    let t0 = Instant::now();

    let sock_addrs: Vec<_> = match addr_str.to_socket_addrs() {
        Ok(addrs) => addrs.collect(),
        Err(e) => return (None, format!("DNS ошибка: {}", e)),
    };

    if sock_addrs.is_empty() {
        return (None, "DNS не вернул IP".to_string());
    }

    let timeout = Duration::from_millis(2000);
    for addr in sock_addrs {
        if let Ok(_stream) = TcpStream::connect_timeout(&addr, timeout) {
            let elapsed_ms = t0.elapsed().as_millis();
            return (Some(elapsed_ms), format!("{} ms ({})", elapsed_ms, addr.ip()));
        }
    }

    (None, "Таймаут соединения".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: String,
    pub text: String,
    pub level: LogLevel,
}

pub enum WorkerCommand {
    Check { full_check: bool },
    Update { files: Vec<ArchiveEntry> },
    Launch,
    PingServers { servers: Vec<ServerInfo> },
}

pub enum WorkerEvent {
    Log(String),
    CheckStage {
        stage: String,
        sub_label: String,
    },
    CheckProgress {
        check_type: String,
        done: usize,
        total: usize,
        current: String,
    },
    CheckFinished { outdated: Vec<ArchiveEntry>, total_bytes: u64 },
    DownloadProgress {
        file_idx: usize,
        file_count: usize,
        bytes_done: u64,
        bytes_total: u64,
        speed_kb_s: f64,
        filename: String,
    },
    UpdateFinished,
    PingResults(Vec<(String, Option<u128>, String)>),
    Error(String),
}

pub struct LauncherApp {
    config: AppConfig,
    status_text: String,
    logs: Vec<LogEntry>,
    is_busy: bool,
    outdated_files: Vec<ArchiveEntry>,
    total_download_bytes: u64,
    progress: f32,
    progress_label: String,
    game_ready: bool,

    server_pings: Arc<Mutex<Vec<(String, Option<u128>, String)>>>,
    show_server_settings: bool,
    new_server_name: String,
    new_server_url: String,

    tx_cmd: Sender<WorkerCommand>,
    rx_event: Receiver<WorkerEvent>,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Modern Gaming Visuals
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(Color32::from_rgb(226, 232, 240));
        visuals.panel_fill = Color32::from_rgb(16, 18, 24);
        visuals.window_fill = Color32::from_rgb(16, 18, 24);
        visuals.extreme_bg_color = Color32::from_rgb(11, 13, 18);
        visuals.faint_bg_color = Color32::from_rgb(24, 28, 38);

        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(24, 28, 38);
        visuals.widgets.noninteractive.rounding = Rounding::same(6.0);
        visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(203, 213, 225);

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 36, 50);
        visuals.widgets.inactive.rounding = Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 54, 76);
        visuals.widgets.hovered.rounding = Rounding::same(6.0);

        visuals.widgets.active.bg_fill = Color32::from_rgb(59, 70, 98);
        visuals.widgets.active.rounding = Rounding::same(6.0);

        visuals.selection.bg_fill = Color32::from_rgb(37, 99, 235);
        cc.egui_ctx.set_visuals(visuals);

        let config = AppConfig::load();
        let (tx_cmd, rx_cmd) = channel::<WorkerCommand>();
        let (tx_event, rx_event) = channel::<WorkerEvent>();

        let cfg_clone = config.clone();
        thread::spawn(move || {
            worker_thread_loop(cfg_clone, rx_cmd, tx_event);
        });

        let mut app = Self {
            config,
            status_text: "Готов к работе".to_string(),
            logs: Vec::new(),
            is_busy: false,
            outdated_files: Vec::new(),
            total_download_bytes: 0,
            progress: 0.0,
            progress_label: String::new(),
            game_ready: false,
            server_pings: Arc::new(Mutex::new(Vec::new())),
            show_server_settings: false,
            new_server_name: String::new(),
            new_server_url: "https://".to_string(),
            tx_cmd,
            rx_event,
        };

        app.add_log("Инициализация open-ew-launcher...", LogLevel::Info);
        app.config.save();

        // Background ping
        let _ = app.tx_cmd.send(WorkerCommand::PingServers {
            servers: app.config.servers.clone(),
        });

        // Auto trigger initial check and update if enabled
        if app.config.auto_update {
            app.start_check(false);
        }

        app
    }

    pub fn start_check(&mut self, full_check: bool) {
        let check_name = if full_check { "Полная проверка (CRC32)" } else { "Быстрая проверка" };
        self.is_busy = true;
        self.progress = 0.0;
        self.status_text = format!("{} — подключение...", check_name);
        self.progress_label = "Загрузка манифеста patchlist.xml с сервера...".to_string();
        self.add_log(format!("Запуск: {}...", check_name), LogLevel::Info);
        let _ = self.tx_cmd.send(WorkerCommand::Check { full_check });
    }

    fn add_log(&mut self, text: impl Into<String>, level: LogLevel) {
        let time = Local::now().format("%H:%M:%S").to_string();
        self.logs.push(LogEntry {
            time,
            text: text.into(),
            level,
        });
        if self.logs.len() > 300 {
            self.logs.remove(0);
        }
    }

    fn check_events(&mut self, ctx: &egui::Context) {
        let mut got_event = false;
        while let Ok(event) = self.rx_event.try_recv() {
            got_event = true;
            match event {
                WorkerEvent::Log(msg) => {
                    let level = if msg.starts_with("ОШИБК") || msg.contains("ошибк") || msg.contains("Ошибка") {
                        LogLevel::Error
                    } else if msg.contains("успешно") || msg.contains("актуальн") || msg.contains("готов к запуску") {
                        LogLevel::Success
                    } else if msg.contains("Предупреждение") || msg.contains("Внимание") {
                        LogLevel::Warning
                    } else {
                        LogLevel::Info
                    };
                    self.add_log(msg, level);
                }
                WorkerEvent::CheckStage { stage, sub_label } => {
                    self.is_busy = true;
                    self.status_text = stage;
                    self.progress = 0.0;
                    self.progress_label = sub_label;
                }
                WorkerEvent::CheckProgress { check_type, done, total, current } => {
                    self.is_busy = true;
                    self.progress = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                    let percent = (self.progress * 100.0) as u32;
                    self.status_text = format!("{} — {}% ({}/{})", check_type, percent, done, total);
                    self.progress_label = format!("Файл: {}", current);
                }
                WorkerEvent::CheckFinished { outdated, total_bytes } => {
                    self.is_busy = false;
                    self.progress = 1.0;
                    self.outdated_files = outdated.clone();
                    self.total_download_bytes = total_bytes;

                    if outdated.is_empty() {
                        self.status_text = "Все файлы актуальны! Клиент готов к запуску.".to_string();
                        self.progress_label = "Клиент проверен, обновлений нет".to_string();
                        self.game_ready = true;
                        self.add_log("Все файлы клиента проверены и соответствуют серверу.", LogLevel::Success);
                    } else {
                        let exe_exists = self.config.game_dir.join("system").join("EndlessWar.exe").exists();
                        let mb = total_bytes as f64 / (1024.0 * 1024.0);
                        let size_str = if mb > 1024.0 {
                            format!("{:.2} ГБ", mb / 1024.0)
                        } else {
                            format!("{:.1} МБ", mb)
                        };

                        self.game_ready = false;
                        if !exe_exists {
                            self.status_text = format!("Требуется установка клиента: {} файлов ({})", outdated.len(), size_str);
                            self.progress_label = format!("Клиент не найден, файлов для загрузки: {}", outdated.len());
                            self.add_log(format!("Клиент не найден в папке. Всего для установки: {} файлов ({}).", outdated.len(), size_str), LogLevel::Warning);
                        } else {
                            self.status_text = format!("Требуется обновление: {} файлов ({})", outdated.len(), size_str);
                            self.progress_label = format!("Найдено изменений: {} файлов", outdated.len());
                            self.add_log(format!("Обнаружено {} устаревших/отсутствующих файлов ({}).", outdated.len(), size_str), LogLevel::Warning);
                        }

                        // AUTO-DOWNLOAD if auto_update is enabled!
                        if self.config.auto_update {
                            self.add_log("Автообновление: запуск скачивания файлов...", LogLevel::Info);
                            let _ = self.tx_cmd.send(WorkerCommand::Update {
                                files: self.outdated_files.clone(),
                            });
                        }
                    }
                }
                WorkerEvent::DownloadProgress {
                    file_idx,
                    file_count,
                    bytes_done,
                    bytes_total,
                    speed_kb_s,
                    filename,
                } => {
                    self.is_busy = true;
                    self.progress = if bytes_total > 0 {
                        bytes_done as f32 / bytes_total as f32
                    } else {
                        file_idx as f32 / file_count.max(1) as f32
                    };

                    let speed_str = if speed_kb_s > 1024.0 {
                        format!("{:.2} МБ/с", speed_kb_s / 1024.0)
                    } else {
                        format!("{:.0} КБ/с", speed_kb_s)
                    };

                    self.progress_label = format!("[{}/{}] {} ({})", file_idx, file_count, filename, speed_str);
                    self.status_text = format!(
                        "Загрузка: {:.1} / {:.1} МБ ({:.0}%)",
                        bytes_done as f64 / 1048576.0,
                        bytes_total as f64 / 1048576.0,
                        self.progress * 100.0
                    );
                }
                WorkerEvent::UpdateFinished => {
                    self.is_busy = false;
                    self.progress = 1.0;
                    self.outdated_files.clear();
                    self.status_text = "Обновление завершено! Клиент готов к запуску.".to_string();
                    self.progress_label = "Все файлы успешно установлены".to_string();
                    self.game_ready = true;
                    self.add_log("Обновление файлов успешно завершено!", LogLevel::Success);
                }
                WorkerEvent::PingResults(results) => {
                    *self.server_pings.lock().unwrap() = results;
                }
                WorkerEvent::Error(err) => {
                    self.is_busy = false;
                    self.status_text = format!("Ошибка: {}", err);
                    self.add_log(format!("ОШИБКА: {}", err), LogLevel::Error);
                }
            }
        }

        if got_event {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_events(ctx);

        if self.is_busy {
            ctx.request_repaint_after(Duration::from_millis(20));
        }

        let footer_frame = egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 36))
            .inner_margin(Margin::symmetric(14.0, 7.0))
            .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 68)));

        egui::TopBottomPanel::bottom("status_footer")
            .frame(footer_frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let cfg_path = AppConfig::config_path();
                    let cfg_display = cfg_path.to_string_lossy().to_string();

                    ui.label(egui::RichText::new("⚙ Конфиг:").size(12.0).color(Color32::from_rgb(160, 175, 195)));
                    let link_resp = ui.add(egui::Link::new(
                        egui::RichText::new(&cfg_display)
                            .size(12.0)
                            .monospace()
                            .color(Color32::from_rgb(147, 197, 253))
                    ));
                    if link_resp.on_hover_text("Нажмите, чтобы открыть папку с файлом конфигурации").clicked() {
                        AppConfig::open_config_folder();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("v0.1.0").size(12.0).strong().color(Color32::from_rgb(160, 175, 195)));
                    });
                });
            });

        let panel_frame = egui::Frame::none()
            .fill(Color32::from_rgb(16, 18, 24))
            .inner_margin(Margin::same(14.0));

        egui::CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

            // ================= HEADER =================
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("Open EndlessWar Launcher").strong().size(21.0).color(Color32::WHITE));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_text = if self.show_server_settings { "✕ Скрыть серверы" } else { "⚙ Серверы и сеть" };
                    if ui.button(btn_text).clicked() {
                        self.show_server_settings = !self.show_server_settings;
                    }

                    let active_name = self.config.servers.iter()
                        .find(|s| s.url == self.config.selected_server)
                        .map(|s| s.name.as_str())
                        .unwrap_or("Сервер");
                    ui.label(egui::RichText::new(format!("🌐 {}", active_name)).small().color(Color32::from_rgb(147, 197, 253)));
                });
            });

            // ================= SERVER SETTINGS ACCORDION =================
            if self.show_server_settings {
                let card_frame = egui::Frame::none()
                    .fill(Color32::from_rgb(24, 28, 38))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(42, 48, 64)))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(Margin::same(12.0));

                card_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Управление серверами обновлений").strong().color(Color32::WHITE));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("⚡ Пинг серверов").clicked() {
                                let _ = self.tx_cmd.send(WorkerCommand::PingServers {
                                    servers: self.config.servers.clone(),
                                });
                            }

                            if ui.button("🎯 Выбрать самый быстрый").clicked() {
                                let pings = self.server_pings.lock().unwrap().clone();
                                if let Some((fastest_url, _, _)) = pings.iter().filter(|(_, p, _)| p.is_some()).min_by_key(|(_, p, _)| p.unwrap()) {
                                    self.config.selected_server = fastest_url.clone();
                                    self.config.save();
                                    self.add_log(format!("Выбран самый быстрый сервер: {}", fastest_url), LogLevel::Success);
                                }
                            }
                        });
                    });

                    ui.separator();

                    let pings = self.server_pings.lock().unwrap().clone();
                    let ping_map: std::collections::HashMap<_, _> = pings.into_iter().map(|(u, p, s)| (u, (p, s))).collect();

                    let mut to_remove = None;
                    let mut new_selected = None;
                    let current_selected = self.config.selected_server.clone();

                    for (i, server) in self.config.servers.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let is_selected = current_selected == server.url;
                            if ui.radio(is_selected, "").clicked() {
                                new_selected = Some(server.url.clone());
                            }

                            ui.add_sized([160.0, 22.0], egui::TextEdit::singleline(&mut server.name));
                            ui.add_sized([ui.available_width() - 170.0, 22.0], egui::TextEdit::singleline(&mut server.url));

                            // Ping badge
                            if let Some((p, s)) = ping_map.get(&server.url) {
                                match p {
                                    Some(ms) if *ms < 100 => {
                                        ui.label(egui::RichText::new(format!("🟢 {} ms", ms)).color(Color32::from_rgb(74, 222, 128)));
                                    }
                                    Some(ms) if *ms < 200 => {
                                        ui.label(egui::RichText::new(format!("🟡 {} ms", ms)).color(Color32::from_rgb(250, 204, 21)));
                                    }
                                    Some(ms) => {
                                        ui.label(egui::RichText::new(format!("🟠 {} ms", ms)).color(Color32::from_rgb(251, 146, 60)));
                                    }
                                    None => {
                                        ui.label(egui::RichText::new(s).color(Color32::from_rgb(248, 113, 113)));
                                    }
                                }
                            } else {
                                ui.label(egui::RichText::new("⌛").color(Color32::GRAY));
                            }

                            if ui.button("🗑").clicked() {
                                to_remove = Some(i);
                            }
                        });
                    }

                    if let Some(sel) = new_selected {
                        self.config.selected_server = sel;
                        self.config.save();
                    }

                    if let Some(idx) = to_remove {
                        if self.config.servers.len() > 1 {
                            self.config.servers.remove(idx);
                            if !self.config.servers.iter().any(|s| s.url == self.config.selected_server) {
                                self.config.selected_server = self.config.servers[0].url.clone();
                            }
                            self.config.save();
                        }
                    }

                    ui.add_space(4.0);
                    // Add new server row
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Добавить:").color(Color32::from_rgb(148, 163, 184)));
                        ui.add_sized([140.0, 22.0], egui::TextEdit::singleline(&mut self.new_server_name).hint_text("Название"));
                        ui.add_sized([ui.available_width() - 95.0, 22.0], egui::TextEdit::singleline(&mut self.new_server_url).hint_text("URL (https://.../updater/)"));
                        if ui.button("➕ Добавить").clicked() {
                            if !self.new_server_url.is_empty() {
                                let name = if self.new_server_name.is_empty() { "Свой сервер".to_string() } else { self.new_server_name.clone() };
                                let url = if !self.new_server_url.ends_with('/') { format!("{}/", self.new_server_url) } else { self.new_server_url.clone() };
                                self.config.servers.push(ServerInfo { name, url: url.clone() });
                                self.config.selected_server = url;
                                self.config.save();
                                self.new_server_name.clear();
                                self.new_server_url = "https://".to_string();
                                let _ = self.tx_cmd.send(WorkerCommand::PingServers { servers: self.config.servers.clone() });
                            }
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Файл конфигурации:").small().color(Color32::from_rgb(148, 163, 184)));
                        let cfg_p = AppConfig::config_path();
                        ui.label(egui::RichText::new(cfg_p.to_string_lossy()).small().monospace().color(Color32::from_rgb(203, 213, 225)));
                        if ui.add(egui::Button::new("📂 Открыть папку").small()).clicked() {
                            AppConfig::open_config_folder();
                        }
                    });
                });
            }

            // ================= GAME FOLDER & OPTIONS CARD =================
            let folder_card = egui::Frame::none()
                .fill(Color32::from_rgb(24, 28, 38))
                .stroke(Stroke::new(1.0, Color32::from_rgb(42, 48, 64)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(12.0));

            folder_card.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📁 Папка игры:").strong().color(Color32::from_rgb(226, 232, 240)));
                    let mut dir_str = self.config.game_dir.to_string_lossy().to_string();
                    let resp = ui.add_sized([ui.available_width() - 105.0, 26.0], egui::TextEdit::singleline(&mut dir_str));
                    if resp.changed() {
                        self.config.game_dir = PathBuf::from(dir_str);
                        self.config.save();
                    }

                    if ui.add_sized([95.0, 26.0], egui::Button::new("Обзор...")).clicked() {
                        if let Some(folder) = rfd::FileDialog::new()
                            .set_directory(&self.config.game_dir)
                            .pick_folder()
                        {
                            self.config.game_dir = folder;
                            self.config.save();
                            self.start_check(false);
                        }
                    }
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.config.auto_update, "Автообновление (проверка и скачивание при старте)").changed() {
                        self.config.save();
                    }

                    let exe_exists = self.config.game_dir.join("system").join("EndlessWar.exe").exists();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if exe_exists {
                            ui.label(egui::RichText::new("✔ EndlessWar.exe обнаружен").small().color(Color32::from_rgb(74, 222, 128)));
                        } else {
                            ui.label(egui::RichText::new("⚠ EndlessWar.exe не найден").small().color(Color32::from_rgb(248, 113, 113)));
                        }
                    });
                });
            });

            // ================= ACTION BUTTONS BAR =================
            ui.horizontal(|ui| {
                let btn_fast = egui::Button::new("🔍 Быстрая проверка")
                    .min_size(egui::vec2(150.0, 38.0))
                    .rounding(6.0);
                let resp_fast = ui.add_enabled(!self.is_busy, btn_fast);
                if resp_fast.on_hover_text("Сверка наличия и размеров файлов (~1-2 сек)").clicked() {
                    self.start_check(false);
                }

                let btn_full = egui::Button::new("🛠 Полная проверка")
                    .min_size(egui::vec2(150.0, 38.0))
                    .rounding(6.0);
                let resp_full = ui.add_enabled(!self.is_busy, btn_full);
                if resp_full.on_hover_text("Полная проверка CRC32 контрольных сумм всех файлов клиента").clicked() {
                    self.start_check(true);
                }

                let exe_exists = self.config.game_dir.join("system").join("EndlessWar.exe").exists();

                if !self.outdated_files.is_empty() {
                    let mb = self.total_download_bytes as f64 / 1048576.0;
                    let label = if !exe_exists {
                        if mb > 1024.0 {
                            format!("▼  Скачать клиент ({:.2} ГБ)", mb / 1024.0)
                        } else {
                            format!("▼  Скачать клиент ({:.1} МБ)", mb)
                        }
                    } else if mb > 1024.0 {
                        format!("▼  Скачать обновления ({:.2} ГБ)", mb / 1024.0)
                    } else {
                        format!("▼  Скачать обновления ({:.1} МБ)", mb)
                    };

                    let btn_update = egui::Button::new(
                        egui::RichText::new(label)
                            .strong()
                            .color(Color32::BLACK)
                    )
                    .fill(Color32::from_rgb(250, 204, 21))
                    .min_size(egui::vec2(165.0, 38.0))
                    .rounding(6.0);

                    if ui.add_enabled(!self.is_busy, btn_update).clicked() {
                        self.is_busy = true;
                        self.progress = 0.0;
                        self.status_text = "Подготовка к скачиванию...".to_string();
                        self.progress_label = "Инициализация...".to_string();
                        let _ = self.tx_cmd.send(WorkerCommand::Update {
                            files: self.outdated_files.clone(),
                        });
                    }
                }

                // PLAY button - right aligned, large & bold green!
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_play = !self.is_busy && exe_exists && (self.game_ready || self.outdated_files.is_empty());
                    let btn_play = egui::Button::new(
                        egui::RichText::new("▶  ИГРАТЬ")
                            .size(17.0)
                            .strong()
                            .color(Color32::WHITE)
                    )
                    .fill(if can_play { Color32::from_rgb(22, 163, 74) } else { Color32::from_rgb(38, 48, 58) })
                    .min_size(egui::vec2(165.0, 38.0))
                    .rounding(6.0);

                    let mut resp_play = ui.add_enabled(can_play, btn_play);
                    if !exe_exists {
                        resp_play = resp_play.on_disabled_hover_text("Клиент игры не найден. Нажмите 'Быстрая проверка' или 'Скачать клиент', чтобы установить игру.");
                    } else if self.is_busy {
                        resp_play = resp_play.on_disabled_hover_text("Выполняется операция (проверка или скачивание)...");
                    } else if !self.outdated_files.is_empty() {
                        resp_play = resp_play.on_disabled_hover_text("Требуется скачать обновления перед запуском игры.");
                    }

                    if can_play && resp_play.clicked() {
                        let _ = self.tx_cmd.send(WorkerCommand::Launch);
                    }
                });
            });

            // ================= PROGRESS & STATUS CARD =================
            let progress_card = egui::Frame::none()
                .fill(Color32::from_rgb(20, 23, 31))
                .stroke(Stroke::new(1.0, Color32::from_rgb(38, 44, 58)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::same(10.0));

            progress_card.show(ui, |ui| {
                ui.horizontal(|ui| {
                    let status_icon = if self.is_busy {
                        "⏳"
                    } else if self.game_ready {
                        "🟢"
                    } else if !self.outdated_files.is_empty() {
                        "🟡"
                    } else {
                        "⚪"
                    };
                    ui.label(egui::RichText::new(format!("{} {}", status_icon, self.status_text)).strong().color(Color32::from_rgb(241, 245, 249)));
                });

                ui.add_space(4.0);

                let pbar = egui::ProgressBar::new(self.progress)
                    .show_percentage()
                    .animate(self.is_busy)
                    .desired_height(14.0)
                    .fill(Color32::from_rgb(59, 130, 246));
                ui.add(pbar);

                if !self.progress_label.is_empty() {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(&self.progress_label).small().color(Color32::from_rgb(148, 163, 184)));
                }
            });

            // ================= EVENT LOG CONSOLE (FULL WIDTH & DYNAMIC HEIGHT) =================
            let log_frame = egui::Frame::none()
                .fill(Color32::from_rgb(12, 14, 18))
                .stroke(Stroke::new(1.0, Color32::from_rgb(38, 44, 58)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(10.0, 8.0));

            let available_height = ui.available_height().max(80.0);

            log_frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📜 Журнал событий").strong().color(Color32::from_rgb(203, 213, 225)));
                    ui.label(egui::RichText::new(format!("({} строк)", self.logs.len())).small().color(Color32::from_rgb(100, 116, 139)));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("🗑 Очистить").small().rounding(4.0)).clicked() {
                            self.logs.clear();
                        }
                    });
                });

                ui.separator();

                // Dynamic vertical stretch and 100% full width
                let scroll_height = (available_height - 38.0).max(50.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(scroll_height)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 3.0;

                        for entry in &self.logs {
                            let color = match entry.level {
                                LogLevel::Error => Color32::from_rgb(248, 113, 113),
                                LogLevel::Success => Color32::from_rgb(74, 222, 128),
                                LogLevel::Warning => Color32::from_rgb(251, 191, 36),
                                LogLevel::Info => Color32::from_rgb(203, 213, 225),
                            };

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                ui.label(egui::RichText::new(&entry.time).monospace().size(13.0).color(Color32::from_rgb(120, 135, 160)));
                                ui.label(egui::RichText::new(&entry.text).monospace().size(13.0).color(color));
                            });
                        }
                    });
            });
        });
    }
}

fn worker_thread_loop(
    mut config: AppConfig,
    rx_cmd: Receiver<WorkerCommand>,
    tx_event: Sender<WorkerEvent>,
) {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    while let Ok(cmd) = rx_cmd.recv() {
        config = AppConfig::load();

        match cmd {
            WorkerCommand::PingServers { servers } => {
                let tx_p = tx_event.clone();
                thread::spawn(move || {
                    let mut results = Vec::new();
                    for s in servers {
                        let (ping_ms, status) = ping_server_tcp(&s.url);
                        results.push((s.url, ping_ms, status));
                    }
                    let _ = tx_p.send(WorkerEvent::PingResults(results));
                });
            }

            WorkerCommand::Check { full_check } => {
                let check_type_str = if full_check { "Полная проверка (CRC32)" } else { "Быстрая проверка" };
                let _ = tx_event.send(WorkerEvent::CheckStage {
                    stage: format!("{} — загрузка манифеста...", check_type_str),
                    sub_label: "Загрузка списка файлов patchlist.xml с сервера...".to_string(),
                });
                let _ = tx_event.send(WorkerEvent::Log(format!("Используется сервер: {}", config.selected_server)));
                let _ = tx_event.send(WorkerEvent::Log("Загрузка манифеста patchlist.xml с сервера...".into()));

                let base_url = format!("{}/base/", config.selected_server.trim_end_matches('/'));
                let ru_url = format!("{}/ru/", config.selected_server.trim_end_matches('/'));

                let base_patchlist_url = format!("{}patchlist.xml", base_url);
                let ru_patchlist_url = format!("{}patchlist.xml", ru_url);

                let mut all_entries = Vec::new();

                match client.get(&base_patchlist_url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(xml) = resp.text() {
                            let entries = parse_patchlist(&xml, &base_url);
                            let _ = tx_event.send(WorkerEvent::Log(format!("Базовый манифест: {} файлов", entries.len())));
                            all_entries.extend(entries);
                        }
                    }
                    Ok(resp) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!("Сервер вернул статус HTTP {}", resp.status())));
                        continue;
                    }
                    Err(e) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!("Не удалось скачать манифест: {}", e)));
                        continue;
                    }
                }

                if config.lang == "ru" {
                    if let Ok(resp) = client.get(&ru_patchlist_url).send() {
                        if resp.status().is_success() {
                            if let Ok(xml) = resp.text() {
                                let entries = parse_patchlist(&xml, &ru_url);
                                let _ = tx_event.send(WorkerEvent::Log(format!("Манифест русской локализации: {} файлов", entries.len())));
                                all_entries.extend(entries);
                            }
                        }
                    }
                }

                if !config.game_dir.exists() {
                    let _ = tx_event.send(WorkerEvent::Log(format!(
                        "Папка '{}' не существует (будет создана автоматически при скачивании).",
                        config.game_dir.display()
                    )));
                }

                let total = all_entries.len();
                let _ = tx_event.send(WorkerEvent::Log(format!("Начало проверки {} файлов клиента...", total)));
                let _ = tx_event.send(WorkerEvent::CheckProgress {
                    check_type: check_type_str.to_string(),
                    done: 0,
                    total,
                    current: "Инициализация проверки файлов...".to_string(),
                });

                let game_dir = config.game_dir.clone();
                let counter = Arc::new(Mutex::new(0usize));
                let last_send = Arc::new(Mutex::new(Instant::now()));
                let outdated = Arc::new(Mutex::new(Vec::new()));
                let tx_progress = tx_event.clone();
                let check_type_owned = check_type_str.to_string();

                all_entries.par_iter().for_each(|entry| {
                    let full_path = game_dir.join(&entry.filename);
                    let mut is_outdated = false;

                    if !full_path.exists() {
                        is_outdated = true;
                    } else if let Ok(meta) = full_path.metadata() {
                        if meta.len() != entry.size {
                            is_outdated = true;
                        } else if full_check {
                            if let Ok(mut f) = File::open(&full_path) {
                                let mut hasher = crc32fast::Hasher::new();
                                let mut buf = [0u8; 65536];
                                while let Ok(n) = f.read(&mut buf) {
                                    if n == 0 { break; }
                                    hasher.update(&buf[..n]);
                                }
                                if hasher.finalize() != entry.original_crc {
                                    is_outdated = true;
                                }
                            } else {
                                is_outdated = true;
                            }
                        }
                    } else {
                        is_outdated = true;
                    }

                    if is_outdated {
                        outdated.lock().unwrap().push(entry.clone());
                    }

                    let mut c = counter.lock().unwrap();
                    *c += 1;
                    let done = *c;
                    drop(c);

                    let mut last = last_send.lock().unwrap();
                    if done == total || done % 5 == 0 || last.elapsed() >= Duration::from_millis(30) {
                        *last = Instant::now();
                        let _ = tx_progress.send(WorkerEvent::CheckProgress {
                            check_type: check_type_owned.clone(),
                            done,
                            total,
                            current: entry.filename.clone(),
                        });
                    }
                });

                let outdated_list = outdated.lock().unwrap().clone();
                let total_bytes: u64 = outdated_list.iter().map(|e| e.size).sum();

                let _ = tx_event.send(WorkerEvent::CheckFinished {
                    outdated: outdated_list,
                    total_bytes,
                });
            }

            WorkerCommand::Update { files } => {
                let total_files = files.len();
                let total_bytes: u64 = files.iter().map(|e| e.size).sum();
                let mut bytes_done: u64 = 0;
                let start_time = Instant::now();

                let _ = tx_event.send(WorkerEvent::Log(format!("Начало скачивания {} файлов...", total_files)));

                let temp_dir = config.game_dir.join(".ew_temp");
                if let Err(e) = fs::create_dir_all(&temp_dir) {
                    let _ = tx_event.send(WorkerEvent::Error(format!(
                        "Не удалось создать директорию {}: {}. Проверьте путь и права доступа.",
                        config.game_dir.display(), e
                    )));
                    continue;
                }

                // Check write permissions in target directory
                let test_path = temp_dir.join(".write_test");
                match File::create(&test_path) {
                    Ok(_) => {
                        let _ = fs::remove_file(test_path);
                    }
                    Err(e) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!(
                            "Нет прав на запись в папку {}: {}. Проверьте права доступа к директории.",
                            config.game_dir.display(), e
                        )));
                        continue;
                    }
                }

                for (idx, entry) in files.iter().enumerate() {
                    let file_url = format!("{}{}.lzma", entry.base_url, entry.filename);
                    let dest_path = config.game_dir.join(&entry.filename);

                    if let Some(parent) = dest_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }

                    let elapsed_secs = start_time.elapsed().as_secs_f64().max(0.001);
                    let speed = (bytes_done as f64 / 1024.0) / elapsed_secs;

                    let _ = tx_event.send(WorkerEvent::DownloadProgress {
                        file_idx: idx + 1,
                        file_count: total_files,
                        bytes_done,
                        bytes_total: total_bytes,
                        speed_kb_s: speed,
                        filename: entry.filename.clone(),
                    });

                    match client.get(&file_url).send() {
                        Ok(mut resp) if resp.status().is_success() => {
                            let temp_lzma = temp_dir.join("current.lzma");
                            let temp_out = temp_dir.join("current.out");

                            if let Ok(mut f_lzma) = File::create(&temp_lzma) {
                                if let Err(e) = resp.copy_to(&mut f_lzma) {
                                    let _ = tx_event.send(WorkerEvent::Error(format!("Ошибка скачивания {}: {}", entry.filename, e)));
                                    continue;
                                }
                            }

                            let decompress_ok = {
                                if let (Ok(f_in), Ok(f_out)) = (File::open(&temp_lzma), File::create(&temp_out)) {
                                    let mut reader = BufReader::new(f_in);
                                    let mut writer = BufWriter::new(f_out);
                                    lzma_rs::lzma_decompress(&mut reader, &mut writer).is_ok()
                                } else {
                                    false
                                }
                            };

                            if decompress_ok {
                                let _ = fs::rename(&temp_out, &dest_path);
                                bytes_done += entry.size;
                            } else {
                                let _ = tx_event.send(WorkerEvent::Error(format!("Ошибка распаковки {}", entry.filename)));
                            }

                            let _ = fs::remove_file(&temp_lzma);
                            let _ = fs::remove_file(&temp_out);
                        }
                        Ok(resp) => {
                            let _ = tx_event.send(WorkerEvent::Error(format!("Ошибка HTTP {} для {}", resp.status(), file_url)));
                        }
                        Err(e) => {
                            let _ = tx_event.send(WorkerEvent::Error(format!("Ошибка соединения {}: {}", entry.filename, e)));
                        }
                    }
                }

                let _ = fs::remove_dir_all(&temp_dir);
                let _ = tx_event.send(WorkerEvent::UpdateFinished);
            }

            WorkerCommand::Launch => {
                let exe_path = config.game_dir.join("system").join("EndlessWar.exe");
                if !exe_path.exists() {
                    let _ = tx_event.send(WorkerEvent::Error(format!("Файл не найден: {}", exe_path.display())));
                    continue;
                }

                let _ = tx_event.send(WorkerEvent::Log(format!("Запуск клиента: {}", exe_path.display())));

                #[cfg(target_os = "windows")]
                let spawn_res = {
                    let working_dir = exe_path.parent().unwrap_or(&config.game_dir);
                    Command::new(&exe_path)
                        .current_dir(working_dir)
                        .spawn()
                };

                #[cfg(not(target_os = "windows"))]
                let spawn_res = {
                    let script_path = std::env::var("HOME")
                        .map(|h| PathBuf::from(h).join("PortProton/data/scripts/start.sh"))
                        .unwrap_or_else(|_| PathBuf::from("/home/cmrd/PortProton/data/scripts/start.sh"));

                    Command::new("portproton")
                        .env("PW_AUTOPLAY", "1")
                        .env("PW_USE_RUNTIME", "0")
                        .arg(&exe_path)
                        .spawn()
                        .or_else(|_| {
                            Command::new("bash")
                                .env("PW_AUTOPLAY", "1")
                                .env("PW_USE_RUNTIME", "0")
                                .arg(&script_path)
                                .arg(&exe_path)
                                .spawn()
                        })
                        .or_else(|_| {
                            Command::new("wine")
                                .arg(&exe_path)
                                .spawn()
                        })
                };

                match spawn_res {
                    Ok(_) => {
                        let _ = tx_event.send(WorkerEvent::Log("Процесс игры успешно запущен!".into()));
                    }
                    Err(e) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!("Не удалось запустить игру: {}", e)));
                    }
                }
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([880.0, 620.0])
            .with_min_inner_size([760.0, 480.0])
            .with_title("Open EndlessWar Launcher"),
        ..Default::default()
    };

    eframe::run_native(
        "Open EndlessWar Launcher",
        native_options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}
