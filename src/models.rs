use crate::config::ServerInfo;

#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub filename: String,
    pub size: u64,
    pub original_crc: u32,
    #[allow(dead_code)]
    pub archive_crc: u32,
    pub base_url: String,
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

pub type ServerPingResult = (String, Option<u128>, String);

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
    CheckFinished {
        outdated: Vec<ArchiveEntry>,
        total_bytes: u64,
    },
    DownloadProgress {
        file_idx: usize,
        file_count: usize,
        bytes_done: u64,
        bytes_total: u64,
        speed_kb_s: f64,
        filename: String,
    },
    UpdateFinished,
    PingResults(Vec<ServerPingResult>),
    Error(String),
}
