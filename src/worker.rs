use crate::config::AppConfig;
use crate::models::{WorkerCommand, WorkerEvent};
use crate::network::{parse_patchlist, ping_server_tcp};
use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read};
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub fn worker_thread_loop(rx_cmd: Receiver<WorkerCommand>, tx_event: Sender<WorkerEvent>) {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    while let Ok(cmd) = rx_cmd.recv() {
        let config = AppConfig::load();

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
                let check_type_str = if full_check {
                    "Полная проверка (CRC32)"
                } else {
                    "Быстрая проверка"
                };
                let _ = tx_event.send(WorkerEvent::CheckStage {
                    stage: format!("{} — загрузка манифеста...", check_type_str),
                    sub_label: "Загрузка списка файлов patchlist.xml с сервера...".to_string(),
                });
                let _ = tx_event.send(WorkerEvent::Log(format!(
                    "Используется сервер: {}",
                    config.selected_server
                )));
                let _ = tx_event.send(WorkerEvent::Log(
                    "Загрузка манифеста patchlist.xml с сервера...".into(),
                ));

                let base_url = format!("{}/base/", config.selected_server.trim_end_matches('/'));
                let ru_url = format!("{}/ru/", config.selected_server.trim_end_matches('/'));

                let base_patchlist_url = format!("{}patchlist.xml", base_url);
                let ru_patchlist_url = format!("{}patchlist.xml", ru_url);

                let mut all_entries = Vec::new();

                match client.get(&base_patchlist_url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(xml) = resp.text() {
                            let entries = parse_patchlist(&xml, &base_url);
                            let _ = tx_event.send(WorkerEvent::Log(format!(
                                "Базовый манифест: {} файлов",
                                entries.len()
                            )));
                            all_entries.extend(entries);
                        }
                    }
                    Ok(resp) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!(
                            "Сервер вернул статус HTTP {}",
                            resp.status()
                        )));
                        continue;
                    }
                    Err(e) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!(
                            "Не удалось скачать манифест: {}",
                            e
                        )));
                        continue;
                    }
                }

                if config.lang == "ru" {
                    if let Ok(resp) = client.get(&ru_patchlist_url).send() {
                        if resp.status().is_success() {
                            if let Ok(xml) = resp.text() {
                                let entries = parse_patchlist(&xml, &ru_url);
                                let _ = tx_event.send(WorkerEvent::Log(format!(
                                    "Манифест русской локализации: {} файлов",
                                    entries.len()
                                )));
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
                let _ = tx_event.send(WorkerEvent::Log(format!(
                    "Начало проверки {} файлов клиента...",
                    total
                )));
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
                                    if n == 0 {
                                        break;
                                    }
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
                    #[allow(clippy::manual_is_multiple_of)]
                    if done == total || done % 5 == 0 || last.elapsed() >= Duration::from_millis(30)
                    {
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

                let _ = tx_event.send(WorkerEvent::Log(format!(
                    "Начало скачивания {} файлов...",
                    total_files
                )));

                let temp_dir = config.game_dir.join(".ew_temp");
                if let Err(e) = fs::create_dir_all(&temp_dir) {
                    let _ = tx_event.send(WorkerEvent::Error(format!(
                        "Не удалось создать директорию {}: {}. Проверьте путь и права доступа.",
                        config.game_dir.display(),
                        e
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
                                    let _ = tx_event.send(WorkerEvent::Error(format!(
                                        "Ошибка скачивания {}: {}",
                                        entry.filename, e
                                    )));
                                    continue;
                                }
                            }

                            let decompress_ok = {
                                if let (Ok(f_in), Ok(f_out)) =
                                    (File::open(&temp_lzma), File::create(&temp_out))
                                {
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
                                let _ = tx_event.send(WorkerEvent::Error(format!(
                                    "Ошибка распаковки {}",
                                    entry.filename
                                )));
                            }

                            let _ = fs::remove_file(&temp_lzma);
                            let _ = fs::remove_file(&temp_out);
                        }
                        Ok(resp) => {
                            let _ = tx_event.send(WorkerEvent::Error(format!(
                                "Ошибка HTTP {} для {}",
                                resp.status(),
                                file_url
                            )));
                        }
                        Err(e) => {
                            let _ = tx_event.send(WorkerEvent::Error(format!(
                                "Ошибка соединения {}: {}",
                                entry.filename, e
                            )));
                        }
                    }
                }

                let _ = fs::remove_dir_all(&temp_dir);
                let _ = tx_event.send(WorkerEvent::UpdateFinished);
            }

            WorkerCommand::Launch => {
                let exe_path = config.game_dir.join("system").join("EndlessWar.exe");
                if !exe_path.exists() {
                    let _ = tx_event.send(WorkerEvent::Error(format!(
                        "Файл не найден: {}",
                        exe_path.display()
                    )));
                    continue;
                }

                let _ = tx_event.send(WorkerEvent::Log(format!(
                    "Запуск клиента: {}",
                    exe_path.display()
                )));

                #[cfg(target_os = "windows")]
                let spawn_res = {
                    let working_dir = exe_path.parent().unwrap_or(&config.game_dir);
                    Command::new(&exe_path).current_dir(working_dir).spawn()
                };

                #[cfg(not(target_os = "windows"))]
                let spawn_res = {
                    let home = std::env::var("HOME").unwrap_or_default();
                    let script_path = PathBuf::from(&home).join("PortProton/data/scripts/start.sh");

                    Command::new("portproton")
                        .env("PW_AUTOPLAY", "1")
                        .env("PW_USE_RUNTIME", "0")
                        .arg(&exe_path)
                        .spawn()
                        .or_else(|_| {
                            if script_path.exists() {
                                Command::new("bash")
                                    .env("PW_AUTOPLAY", "1")
                                    .env("PW_USE_RUNTIME", "0")
                                    .arg(&script_path)
                                    .arg(&exe_path)
                                    .spawn()
                            } else {
                                Err(std::io::Error::new(
                                    std::io::ErrorKind::NotFound,
                                    "PortProton script not found",
                                ))
                            }
                        })
                        .or_else(|_| Command::new("wine").arg(&exe_path).spawn())
                };

                match spawn_res {
                    Ok(_) => {
                        let _ =
                            tx_event.send(WorkerEvent::Log("Процесс игры успешно запущен!".into()));
                    }
                    Err(e) => {
                        let _ = tx_event.send(WorkerEvent::Error(format!(
                            "Не удалось запустить игру: {}",
                            e
                        )));
                    }
                }
            }
        }
    }
}
