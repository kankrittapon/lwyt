#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use eframe::egui::{
    self, Button, CentralPanel, Color32, FontData, FontDefinitions, FontFamily, RichText,
    ScrollArea, Slider, Stroke, TextEdit, TopBottomPanel, Vec2, ViewportBuilder,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MPV_PATH: &str = r"C:\Program Files\MPV Player\mpv.exe";
const DEFAULT_URL: &str =
    "https://www.youtube.com/watch?v=liTj2cga-X8&list=PLTubEPwWLaT7_rDszOkDaj57rF02u3SZu";
const STATE_FILE: &str = "config.json";
const API_PORT: u16 = 9733;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("LYTBokkChoYx")
            .with_inner_size(Vec2::new(760.0, 520.0))
            .with_min_inner_size(Vec2::new(600.0, 420.0))
            .with_resizable(true),
        ..Default::default()
    };

    let _result = eframe::run_native(
        "LYTBokkChoYx",
        options,
        Box::new(|cc| Ok(Box::new(PlayerApp::new(cc)))),
    );

    // eframe::run_native returns after the window is closed and on_exit has
    // finished saving session state + history to disk. At this point it is safe
    // to force-exit to terminate lingering background threads (IPC poller, etc.).
    std::process::exit(0);
}

fn unique_id() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackState {
    Stopped,
    Loading,
    Playing,
    Paused,
    Error,
}

impl PlaybackState {
    fn label(self) -> &'static str {
        match self {
            Self::Stopped => "STOPPED",
            Self::Loading => "LOADING",
            Self::Playing => "PLAYING",
            Self::Paused => "PAUSED",
            Self::Error => "ERROR",
        }
    }
}

struct MpvController {
    child: Child,
    ipc_path: String,
    events: Receiver<MpvEvent>,
}

impl MpvController {
    fn start(url: &str, volume: f32, playlist_start: Option<usize>) -> Result<Self, String> {
        if !Path::new(MPV_PATH).exists() {
            return Err(format!("MPV was not found at {MPV_PATH}"));
        }

        let ipc_path = format!(
            r"\\.\pipe\lytbokkchoyx-{}-{}",
            std::process::id(),
            unique_id()
        );
        let ipc_arg = format!("--input-ipc-server={ipc_path}");
        let mut args = vec![
            "--no-video".to_owned(),
            "--vo=null".to_owned(),
            "--ytdl-format=bestaudio".to_owned(),
            "--demuxer-max-bytes=32M".to_owned(),
            "--demuxer-max-back-bytes=10M".to_owned(),
            "--force-window=no".to_owned(),
            "--idle=no".to_owned(),
            "--input-terminal=no".to_owned(),
            ipc_arg,
            "--term-playing-msg=APP_EVENT title=${media-title} playlist=${playlist-pos-1}"
                .to_owned(),
            "--term-status-msg=APP_STATE pause=${pause} percent=${percent-pos} time=${time-pos} duration=${duration} playlist=${playlist-pos-1}"
                .to_owned(),
            format!("--volume={}", volume.round()),
        ];
        if let Some(index) = playlist_start {
            args.push(format!("--playlist-start={index}"));
        }
        args.push(url.to_owned());

        let mut command = hidden_command(MPV_PATH);
        let mut child = command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("Failed to start MPV: {err}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to open MPV output".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Failed to open MPV error output".to_owned())?;
        let (tx, events) = mpsc::channel();

        let out_tx = tx.clone();
        let poll_tx = tx.clone();
        let poll_ipc_path = ipc_path.clone();
        thread::spawn(move || read_mpv_stream(stdout, out_tx));
        thread::spawn(move || read_mpv_stream(stderr, tx));
        thread::spawn(move || poll_mpv_ipc(poll_ipc_path, poll_tx));

        Ok(Self {
            child,
            ipc_path,
            events,
        })
    }

    fn send_command(&mut self, command: serde_json::Value) -> Result<(), String> {
        send_ipc_command(&self.ipc_path, command).map(|_| ())
    }

    fn set_pause(&mut self, paused: bool) -> Result<(), String> {
        self.send_command(json!(["set_property", "pause", paused]))
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.send_command(json!(["set_property", "volume", volume.round()]))
    }

    fn seek_absolute(&mut self, seconds: f64) -> Result<(), String> {
        self.send_command(json!(["seek", seconds.max(0.0), "absolute"]))
    }

    fn playlist_next(&mut self) -> Result<(), String> {
        self.send_command(json!(["playlist-next"]))
    }

    fn playlist_prev(&mut self) -> Result<(), String> {
        self.send_command(json!(["playlist-prev"]))
    }

    fn poll_exit(&mut self) -> Result<Option<i32>, String> {
        self.child
            .try_wait()
            .map(|status| status.map(|value| value.code().unwrap_or_default()))
            .map_err(|err| format!("Failed to read MPV state: {err}"))
    }

    fn drain_events(&mut self) -> Vec<MpvEvent> {
        self.events.try_iter().collect()
    }

    fn stop(&mut self) {
        let _ = self.send_command(json!(["quit"]));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for MpvController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug)]
enum MpvEvent {
    State {
        paused: Option<bool>,
        time_pos: Option<f64>,
        duration: Option<f64>,
        playlist_index: Option<usize>,
        media_title: Option<String>,
    },
    NowPlaying {
        title: String,
        playlist_index: Option<usize>,
    },
    Log(String),
    IpcError(String),
}

#[derive(Clone, Debug)]
struct PlaylistItem {
    title: String,
    url: String,
    duration: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HistoryItem {
    url: String,
    title: String,
    last_position: f64,
    duration: Option<f64>,
    last_played: u64,
}

struct Chapter {
    title: String,
    start_time: f64,
}

#[derive(Deserialize)]
struct YtdlpItem {
    title: Option<String>,
    url: Option<String>,
    webpage_url: Option<String>,
    duration: Option<f64>,
    chapters: Option<Vec<YtdlpChapter>>,
}

#[derive(Deserialize)]
struct YtdlpChapter {
    title: Option<String>,
    start_time: Option<f64>,
}

struct LoadResult {
    id: u64,
    original_input_url: String,
    playlist: Vec<PlaylistItem>,
    selected_track: Option<usize>,
    resume_position: Option<f64>,
    chapters: Vec<Chapter>,
    playback: Result<MpvController, String>,
    logs: Vec<String>,
    append_to_queue: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SessionState {
    original_input_url: String,
    current_track_index: usize,
    playback_position_seconds: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct LegacySessionState {
    current_url: String,
    active_playlist_index: Option<usize>,
    playback_position_seconds: f64,
}

struct PlayerApp {
    url: String,
    original_input_url: String,
    state: PlaybackState,
    status: String,
    mpv: Option<MpvController>,
    volume: f32,
    seek_position: f64,
    seek_input_buffer: String,
    time_pos: f64,
    duration: f64,
    user_seeking: bool,
    last_poll: Instant,
    playlist: Vec<PlaylistItem>,
    selected_track: Option<usize>,
    now_playing_title: String,
    chapters: Vec<Chapter>,
    show_debug: bool,
    debug_logs: VecDeque<String>,
    debug_export_logs: VecDeque<String>,
    opacity: f32,
    load_tx: mpsc::Sender<LoadResult>,
    load_rx: Receiver<LoadResult>,
    load_id: u64,
    loading: bool,
    history: Vec<HistoryItem>,
    active_tab: usize,
    last_history_save: Instant,
    last_session_save: Instant,
    reconnect_attempts: usize,
    api_rx: Receiver<String>,
}

impl PlayerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_unicode_fonts(&cc.egui_ctx);
        apply_style(&cc.egui_ctx, 1.0);
        let (load_tx, load_rx) = mpsc::channel();
        let (api_tx, api_rx) = mpsc::channel::<String>();

        // Start lightweight HTTP API server for external queue management (n8n, LINE, etc.)
        start_api_server(api_tx.clone());
        start_remote_queue_poller(api_tx);

        let saved_state = load_session_state();
        let history = load_history();
        let mut app = Self {
            url: DEFAULT_URL.to_owned(),
            original_input_url: DEFAULT_URL.to_owned(),
            state: PlaybackState::Stopped,
            status: "Ready. Paste an audio, stream, YouTube, or playlist URL.".to_owned(),
            mpv: None,
            volume: 70.0,
            seek_position: 0.0,
            seek_input_buffer: format_duration(0.0),
            time_pos: 0.0,
            duration: 0.0,
            user_seeking: false,
            last_poll: Instant::now(),
            playlist: Vec::new(),
            selected_track: None,
            now_playing_title: String::new(),
            chapters: Vec::new(),
            show_debug: false,
            debug_logs: VecDeque::with_capacity(60),
            debug_export_logs: VecDeque::with_capacity(500),
            opacity: 1.0,
            load_tx,
            load_rx,
            load_id: 0,
            loading: false,
            history,
            active_tab: 0,
            last_history_save: Instant::now(),
            last_session_save: Instant::now(),
            reconnect_attempts: 0,
            api_rx,
        };

        if let Some(state) = saved_state {
            let (cleaned_url, _) = extract_time_from_url(&state.original_input_url);
            app.url = cleaned_url.clone();
            app.original_input_url = cleaned_url.clone();
            app.seek_position = state.playback_position_seconds.max(0.0);
            app.seek_input_buffer = format_duration(app.seek_position);
            app.time_pos = app.seek_position;
            app.selected_track = Some(state.current_track_index);
            app.status = format!(
                "Restoring previous session at {}...",
                format_duration(app.seek_position)
            );
            app.start_load_job(
                cleaned_url,
                true,
                Some(state.current_track_index),
                Some(state.playback_position_seconds),
                None,
                false,
            );
        }

        app
    }

    fn load(&mut self) {
        self.reconnect_attempts = 0;
        let url = self.url.trim().to_owned();
        if url.is_empty() {
            self.fail("Enter a URL before loading.");
            return;
        }

        // Extract timestamp from URL to avoid seeking conflict in MPV
        let (cleaned_url, extracted_time) = extract_time_from_url(&url);
        self.url = cleaned_url.clone();
        self.original_input_url = cleaned_url.clone();

        let resume_pos = if let Some(time) = extracted_time {
            self.seek_position = time;
            self.seek_input_buffer = format_duration(time);
            self.time_pos = time;
            Some(time)
        } else {
            self.get_last_position(&cleaned_url)
        };

        self.start_load_job(cleaned_url, true, None, resume_pos, None, false);
    }

    fn start_load_job(
        &mut self,
        original_input_url: String,
        fetch_playlist: bool,
        selected_track: Option<usize>,
        resume_position: Option<f64>,
        playback_url: Option<String>,
        append_to_queue: bool,
    ) {
        if !append_to_queue {
            self.stop();
            if fetch_playlist {
                self.playlist.clear();
            }
            self.chapters.clear();
            self.selected_track = selected_track;
            self.state = PlaybackState::Loading;
            self.status = "Loading metadata and starting MPV...".to_owned();
        } else {
            self.status = "Fetching metadata to add to queue...".to_owned();
        }

        self.load_id = self.load_id.saturating_add(1);
        let id = self.load_id;
        let volume = self.volume;
        let tx = self.load_tx.clone();
        self.loading = true;

        self.push_debug(format!(
            "Starting background load job {id} (append={append_to_queue}): {original_input_url}"
        ));

        thread::spawn(move || {
            let (cleaned_input_url, extracted_time) = extract_time_from_url(&original_input_url);

            let (playlist, metadata_logs) = if fetch_playlist {
                fetch_playlist_items(&cleaned_input_url)
            } else {
                (
                    Vec::new(),
                    vec!["Skipped metadata refresh for selected queue item.".to_owned()],
                )
            };
            let resolved_selected_track = selected_track.map(|index| {
                if playlist.is_empty() {
                    index
                } else {
                    index.min(playlist.len().saturating_sub(1))
                }
            });

            let playback_uses_original_playlist = playback_url.is_none();
            let playback_target = playback_url.unwrap_or_else(|| original_input_url.clone());
            let (cleaned_playback_target, extracted_play_time) =
                extract_time_from_url(&playback_target);

            let final_resume_position = resume_position.or(extracted_time).or(extracted_play_time);

            let (chapters, chapter_logs, playback) = if append_to_queue {
                (Vec::new(), Vec::new(), Err("Queue append mode".to_owned()))
            } else {
                let chapter_target = resolved_selected_track
                    .and_then(|index| playlist.get(index))
                    .map(|item| item.url.as_str())
                    .unwrap_or(&cleaned_playback_target);
                let (chaps, chaps_logs) = fetch_chapter_items(chapter_target);
                let pb = MpvController::start(
                    &cleaned_playback_target,
                    volume,
                    if playback_uses_original_playlist {
                        resolved_selected_track
                    } else {
                        None
                    },
                );
                (chaps, chaps_logs, pb)
            };

            let mut logs = metadata_logs;
            logs.extend(chapter_logs);

            let _ = tx.send(LoadResult {
                id,
                original_input_url: cleaned_input_url,
                playlist,
                selected_track: resolved_selected_track,
                resume_position: final_resume_position,
                chapters,
                playback,
                logs,
                append_to_queue,
            });
        });
    }

    fn stop(&mut self) {
        // Save current track progress to history before clearing state
        if self.state == PlaybackState::Playing || self.state == PlaybackState::Paused {
            if self.time_pos > 0.0 {
                if let Some(track_idx) = self.selected_track {
                    if let Some(item) = self.playlist.get(track_idx).cloned() {
                        self.update_history_item(
                            &item.url,
                            &item.title,
                            self.time_pos,
                            Some(self.duration),
                            true,
                        );
                    }
                }
            }
        }

        // Save session state (URL + track + position) BEFORE resetting fields,
        // so we can restore the exact resume point on next launch.
        self.save_session_state();

        if let Some(mut mpv) = self.mpv.take() {
            mpv.stop();
        }
        self.seek_position = 0.0;
        self.seek_input_buffer = format_duration(0.0);
        self.time_pos = 0.0;
        self.duration = 0.0;
        self.user_seeking = false;
        self.state = PlaybackState::Stopped;
        self.status = "Stopped.".to_owned();
        self.loading = false;
    }

    fn toggle_pause(&mut self) {
        match self.state {
            PlaybackState::Playing | PlaybackState::Paused => {
                let target_pause = self.state == PlaybackState::Playing;
                if let Some(mpv) = self.mpv.as_mut() {
                    match mpv.set_pause(target_pause) {
                        Ok(()) => {
                            self.state = if target_pause {
                                PlaybackState::Paused
                            } else {
                                PlaybackState::Playing
                            };
                            self.status = if target_pause {
                                "Pause requested.".to_owned()
                            } else {
                                "Resume requested.".to_owned()
                            };
                        }
                        Err(err) => self.fail(err),
                    }
                } else {
                    self.state = PlaybackState::Stopped;
                    self.status = "No active MPV process.".to_owned();
                }
            }
            PlaybackState::Stopped => {
                self.status =
                    "Stopped. Press Load or choose a playlist item to start playback.".to_owned();
            }
            PlaybackState::Loading => {
                self.status =
                    "Still loading. Pause is unavailable until playback starts.".to_owned();
            }
            PlaybackState::Error => {
                self.status = "Resolve the current error, then press Load.".to_owned();
            }
        }
    }

    fn command(&mut self, mpv_command: &str, success: &str) {
        if let Some(mpv) = self.mpv.as_mut() {
            let result = match mpv_command {
                "playlist-prev" => mpv.playlist_prev(),
                "playlist-next" => mpv.playlist_next(),
                _ => Err(format!("Unsupported MPV command: {mpv_command}")),
            };
            match result {
                Ok(()) => self.status = success.to_owned(),
                Err(err) => self.fail(err),
            }
        } else {
            self.status = "Load a URL first.".to_owned();
        }
    }

    fn set_volume(&mut self) {
        if let Some(mpv) = self.mpv.as_mut() {
            if let Err(err) = mpv.set_volume(self.volume) {
                self.fail(err);
            }
        }
    }

    fn seek(&mut self) {
        if self.state == PlaybackState::Stopped || self.mpv.is_none() {
            self.status = "Load a URL before seeking.".to_owned();
            return;
        }

        if self.duration > 0.0 {
            self.seek_position = self.seek_position.clamp(0.0, self.duration);
        } else {
            self.seek_position = self.seek_position.max(0.0);
        }

        if let Some(mpv) = self.mpv.as_mut() {
            if let Err(err) = mpv.seek_absolute(self.seek_position) {
                self.fail(err);
                return;
            }
        }
        self.status = "Seek requested.".to_owned();
        self.time_pos = self.seek_position.max(0.0);
        self.seek_input_buffer = format_duration(self.seek_position);
    }

    fn seek_chapter(&mut self, seconds: f64) {
        self.seek_position = seconds.max(0.0);
        self.seek();
    }

    fn poll_api_queue(&mut self) {
        while let Ok(url) = self.api_rx.try_recv() {
            let url = url.trim().to_owned();
            if url.is_empty() {
                continue;
            }
            self.push_debug(format!("API: Queuing URL: {url}"));
            let (cleaned_url, _) = extract_time_from_url(&url);

            if self.state == PlaybackState::Stopped && self.mpv.is_none() {
                // Nothing playing → load and start immediately
                self.url = cleaned_url.clone();
                self.original_input_url = cleaned_url.clone();
                let resume_pos = self.get_last_position(&cleaned_url);
                self.start_load_job(cleaned_url, true, None, resume_pos, None, false);
            } else {
                // Already playing → append to queue
                self.start_load_job(cleaned_url, true, None, None, None, true);
            }
        }
    }

    fn poll_mpv(&mut self) {
        self.poll_load_jobs();
        self.poll_api_queue();

        // Update history and remember last played position in memory
        if self.state == PlaybackState::Playing && self.time_pos > 0.0 {
            if let Some(track_idx) = self.selected_track {
                if let Some(item) = self.playlist.get(track_idx).cloned() {
                    let should_save_to_disk =
                        self.last_history_save.elapsed() > Duration::from_secs(5);
                    self.update_history_item(
                        &item.url,
                        &item.title,
                        self.time_pos,
                        Some(self.duration),
                        should_save_to_disk,
                    );
                    if should_save_to_disk {
                        self.last_history_save = Instant::now();
                    }
                }
            }
        }

        // Periodic session state save (every 30s) as crash-safety net
        if (self.state == PlaybackState::Playing || self.state == PlaybackState::Paused)
            && self.time_pos > 0.0
            && self.last_session_save.elapsed() > Duration::from_secs(30)
        {
            self.save_session_state();
            self.last_session_save = Instant::now();
        }

        if self.last_poll.elapsed() < Duration::from_millis(750) {
            return;
        }

        self.last_poll = Instant::now();
        let Some(mpv) = self.mpv.as_mut() else {
            return;
        };

        let events = mpv.drain_events();
        let exit = mpv.poll_exit();

        for event in events {
            self.handle_mpv_event(event);
        }

        match exit {
            Ok(Some(code)) => {
                self.mpv = None;

                // Save current position before resetting
                if self.time_pos > 0.0 {
                    if let Some(track_idx) = self.selected_track {
                        if let Some(item) = self.playlist.get(track_idx).cloned() {
                            self.update_history_item(
                                &item.url,
                                &item.title,
                                self.time_pos,
                                Some(self.duration),
                                true,
                            );
                        }
                    }
                }

                if code == 0 {
                    self.state = PlaybackState::Stopped;
                    self.status = format!("MPV exited with code {code}.");
                    // Auto-advance: Play next track if playing finished successfully
                    if let Some(current_idx) = self.selected_track {
                        let next_idx = current_idx + 1;
                        if next_idx < self.playlist.len() {
                            self.play_track(next_idx);
                        }
                    }
                } else {
                    // MPV exited with non-zero code. Attempt to reconnect if we were playing/paused.
                    if self.state == PlaybackState::Playing || self.state == PlaybackState::Paused {
                        if self.reconnect_attempts < 3 {
                            if let Some(current_idx) = self.selected_track {
                                if let Some(item) = self.playlist.get(current_idx).cloned() {
                                    self.reconnect_attempts += 1;
                                    self.status = format!(
                                        "MPV exited unexpectedly. Reconnecting (attempt {}/3)...",
                                        self.reconnect_attempts
                                    );
                                    self.push_debug(format!(
                                        "Auto-reconnecting to {} at position {}s...",
                                        item.url, self.time_pos
                                    ));

                                    let resume_pos = Some(self.time_pos);
                                    self.start_load_job(
                                        self.original_input_url.clone(),
                                        false,
                                        Some(current_idx),
                                        resume_pos,
                                        Some(item.url),
                                        false,
                                    );
                                    return;
                                }
                            }
                        } else {
                            self.fail(
                                "MPV exited unexpectedly. Maximum reconnection attempts reached.",
                            );
                            return;
                        }
                    }
                    self.state = PlaybackState::Stopped;
                    self.status = format!("MPV exited with code {code}.");
                }
            }
            Ok(None) => {}
            Err(err) => self.fail(err),
        }
    }

    fn poll_load_jobs(&mut self) {
        while let Ok(result) = self.load_rx.try_recv() {
            if result.id != self.load_id {
                self.push_debug(format!("Ignored stale load job {}", result.id));
                continue;
            }

            for log in result.logs {
                self.push_debug(log);
            }

            self.loading = false;

            if result.append_to_queue {
                if !result.playlist.is_empty() {
                    let old_len = self.playlist.len();
                    self.playlist.extend(result.playlist);
                    self.status = format!(
                        "Added items to Play Queue. Total: {} items.",
                        self.playlist.len()
                    );

                    // Start playing if nothing is currently active
                    if self.state == PlaybackState::Stopped && self.mpv.is_none() {
                        self.selected_track = Some(old_len);
                        if let Some(item) = self.playlist.get(old_len) {
                            let resume_pos = self.get_last_position(&item.url);
                            self.start_load_job(
                                item.url.clone(),
                                false,
                                Some(old_len),
                                resume_pos,
                                None,
                                false,
                            );
                        }
                    }
                } else {
                    self.status = "No tracks found to add to queue.".to_owned();
                }
                continue;
            }

            self.url = result.original_input_url.clone();
            self.original_input_url = result.original_input_url;
            if !result.playlist.is_empty() {
                self.playlist = result.playlist;
            }
            self.chapters = result.chapters;
            self.selected_track = result.selected_track.or(Some(0));

            match result.playback {
                Ok(controller) => {
                    self.reconnect_attempts = 0; // Successfully established connection to a new MPV instance
                    self.mpv = Some(controller);
                    if let Some(position) = result.resume_position {
                        let position = position.max(0.0);
                        self.seek_position = position;
                        self.seek_input_buffer = format_duration(position);
                        self.time_pos = position;
                        if let Some(mpv) = self.mpv.as_mut() {
                            match mpv.seek_absolute(position) {
                                Ok(()) => self.push_debug(format!(
                                    "Resume seek requested at {}.",
                                    format_duration(position)
                                )),
                                Err(err) => self.push_debug(format!("Resume seek failed: {err}")),
                            }
                        }
                    }
                    self.state = PlaybackState::Playing;
                    self.status = format!(
                        "Loaded {} playlist item(s). Playback started.",
                        self.playlist.len()
                    );

                    // Register initial play history
                    if let Some(track_idx) = self.selected_track {
                        if let Some(item) = self.playlist.get(track_idx) {
                            let url = item.url.clone();
                            let title = item.title.clone();
                            let duration = item.duration;
                            self.update_history_item(&url, &title, self.time_pos, duration, true);
                        }
                    }
                }
                Err(err) => self.fail(err),
            }
        }
    }

    fn handle_mpv_event(&mut self, event: MpvEvent) {
        match event {
            MpvEvent::State {
                paused,
                time_pos,
                duration,
                playlist_index,
                media_title,
            } => {
                if let Some(value) = paused {
                    self.state = if value {
                        PlaybackState::Paused
                    } else {
                        PlaybackState::Playing
                    };
                }
                if let Some(value) = duration {
                    self.duration = value.max(0.0);
                }
                if let Some(value) = time_pos {
                    self.time_pos = value.max(0.0);
                }
                if !self.user_seeking {
                    self.seek_position = self.time_pos;
                }
                if let Some(value) = playlist_index {
                    self.selected_track = Some(value);
                }
                if let Some(title) = media_title.filter(|title| !title.is_empty()) {
                    if self.now_playing_title != title {
                        self.now_playing_title = title;
                        self.status = format!("Now playing: {}", self.now_playing_title);
                    }
                }
            }
            MpvEvent::NowPlaying {
                title,
                playlist_index,
            } => {
                if !title.is_empty() {
                    self.now_playing_title = title;
                    self.status = format!("Now playing: {}", self.now_playing_title);
                }
                if let Some(value) = playlist_index {
                    self.selected_track = Some(value);
                }
            }
            MpvEvent::Log(line) => self.push_debug(line),
            MpvEvent::IpcError(err) => {
                self.push_debug(format!("MPV IPC connection lost: {err}"));

                // Save current position immediately
                if self.time_pos > 0.0 {
                    if let Some(track_idx) = self.selected_track {
                        if let Some(item) = self.playlist.get(track_idx).cloned() {
                            self.update_history_item(
                                &item.url,
                                &item.title,
                                self.time_pos,
                                Some(self.duration),
                                true,
                            );
                        }
                    }
                }

                if self.state == PlaybackState::Playing || self.state == PlaybackState::Paused {
                    if self.reconnect_attempts < 3 {
                        if let Some(current_idx) = self.selected_track {
                            if let Some(item) = self.playlist.get(current_idx).cloned() {
                                self.reconnect_attempts += 1;
                                self.status = format!(
                                    "IPC connection lost. Reconnecting (attempt {}/3)...",
                                    self.reconnect_attempts
                                );
                                self.push_debug(format!(
                                    "Auto-reconnecting to {} at position {}s...",
                                    item.url, self.time_pos
                                ));

                                let resume_pos = Some(self.time_pos);
                                self.start_load_job(
                                    self.original_input_url.clone(),
                                    false,
                                    Some(current_idx),
                                    resume_pos,
                                    Some(item.url),
                                    false,
                                );
                                return;
                            }
                        }
                    } else {
                        self.fail("MPV connection lost. Maximum reconnection attempts reached.");
                    }
                }

                // If not playing/paused or no reconnect was triggered, stop MPV controller cleanly
                self.stop();
            }
        }
    }

    fn play_track(&mut self, index: usize) {
        self.reconnect_attempts = 0;
        let Some(item) = self.playlist.get(index) else {
            return;
        };
        let url = item.url.clone();
        self.selected_track = Some(index);
        let resume_pos = self.get_last_position(&url);
        self.start_load_job(
            self.original_input_url.clone(),
            false,
            Some(index),
            resume_pos,
            Some(url),
            false,
        );
    }

    fn fail(&mut self, message: impl Into<String>) {
        self.state = PlaybackState::Error;
        self.status = message.into();
        self.mpv = None;
        self.loading = false;
    }

    fn push_debug(&mut self, line: impl Into<String>) {
        let line = line.into();
        if self.debug_logs.len() >= 60 {
            self.debug_logs.pop_front();
        }
        if self.debug_export_logs.len() >= 500 {
            self.debug_export_logs.pop_front();
        }
        self.debug_logs.push_back(line.clone());
        self.debug_export_logs.push_back(line);
    }

    fn export_debug_logs(&mut self) {
        let path = Path::new("lytbokkchoyx_debug.log");
        let contents = if self.debug_export_logs.is_empty() {
            "No debug logs captured.\n".to_owned()
        } else {
            let lines: Vec<&str> = self.debug_export_logs.iter().map(|s| s.as_str()).collect();
            format!("{}\n", lines.join("\n"))
        };

        match fs::write(path, contents) {
            Ok(()) => self.status = format!("Exported logs to {}.", path.display()),
            Err(err) => self.status = format!("Failed to export logs: {err}"),
        }
    }

    fn current_session_state(&self) -> Option<SessionState> {
        let url = self.original_input_url.trim();
        if url.is_empty() {
            return None;
        }

        Some(SessionState {
            original_input_url: url.to_owned(),
            current_track_index: self.selected_track.unwrap_or(0),
            playback_position_seconds: self.time_pos.max(0.0),
        })
    }

    fn save_session_state(&mut self) {
        if let Some(state) = self.current_session_state() {
            match save_session_state(&state) {
                Ok(()) => self.push_debug(format!("Saved playback state to {STATE_FILE}.")),
                Err(err) => self.push_debug(format!("Failed to save playback state: {err}")),
            }
        }
    }
}

impl eframe::App for PlayerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // IMPORTANT: Save session state BEFORE stop(), because stop() resets
        // time_pos and seek_position to 0.0, which would lose the resume position.
        let _ = save_history(&self.history);
        self.save_session_state();
        self.stop(); // Stop MPV and terminate sub-processes cleanly
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_mpv();
        // Use faster repaint only when actively playing; otherwise save CPU/RAM
        let repaint_interval = if self.state == PlaybackState::Playing || self.loading {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(750)
        };
        ctx.request_repaint_after(repaint_interval);
        apply_style(ctx, self.opacity);

        TopBottomPanel::top("display").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("LYTBokkChoYx")
                        .size(22.0)
                        .strong()
                        .color(Color32::from_rgb(245, 190, 68)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_sized(
                            [104.0, 24.0],
                            Button::new(if self.show_debug {
                                "Hide Debug"
                            } else {
                                "Toggle Debug"
                            }),
                        )
                        .clicked()
                    {
                        self.show_debug = !self.show_debug;
                    }
                    ui.label(
                        RichText::new(self.state.label())
                            .monospace()
                            .color(status_color(self.state)),
                    );
                });
            });
            ui.label(
                RichText::new(&self.status)
                    .monospace()
                    .color(Color32::from_rgb(165, 230, 160)),
            );
            ui.add_space(8.0);
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.url)
                        .hint_text("Audio, stream, YouTube, or playlist URL")
                        .desired_width(f32::INFINITY),
                );
            });

            ui.add_space(10.0);
            ui.columns(2, |columns| {
                columns[0].vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.loading,
                                Button::new(if self.loading { "Loading" } else { "Load" }),
                            )
                            .clicked()
                        {
                            self.load();
                        }

                        if ui
                            .add_enabled(!self.loading, Button::new("+ Queue"))
                            .clicked()
                        {
                            let url = self.url.trim().to_owned();
                            if !url.is_empty() {
                                let (cleaned_url, _) = extract_time_from_url(&url);
                                self.start_load_job(cleaned_url, true, None, None, None, true);
                            } else {
                                self.status = "Enter a URL to add to queue.".to_owned();
                            }
                        }

                        let can_toggle =
                            matches!(self.state, PlaybackState::Playing | PlaybackState::Paused)
                                && self.mpv.is_some();
                        let label = match self.state {
                            PlaybackState::Paused => "Play",
                            PlaybackState::Loading => "...",
                            _ => "Pause",
                        };
                        if ui.add_enabled(can_toggle, Button::new(label)).clicked() {
                            self.toggle_pause();
                        }
                        if ui.add_sized([72.0, 34.0], Button::new("Stop")).clicked() {
                            self.stop();
                        }
                    });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add_sized([68.0, 34.0], Button::new("Prev")).clicked() {
                            self.command("playlist-prev", "Previous track requested.");
                        }
                        if ui.add_sized([68.0, 34.0], Button::new("Next")).clicked() {
                            self.command("playlist-next", "Next track requested.");
                        }
                    });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("VOL").monospace());
                        let changed = ui
                            .add(Slider::new(&mut self.volume, 0.0..=100.0).show_value(true))
                            .changed();
                        if changed {
                            self.set_volume();
                        }
                    });

                    ui.label(
                        RichText::new(format!(
                            "Now: {} / {}",
                            format_duration(self.time_pos),
                            if self.duration > 0.0 {
                                format_duration(self.duration)
                            } else {
                                "--:--".to_owned()
                            }
                        ))
                        .small()
                        .monospace()
                        .color(Color32::from_rgb(210, 215, 225)),
                    );

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("SEEK").monospace());
                        let seek_max = self.duration.max(1.0);
                        let response = ui.add_enabled(
                            self.mpv.is_some(),
                            Slider::new(&mut self.seek_position, 0.0..=seek_max)
                                .custom_formatter(|value, _| format_duration(value))
                                .custom_parser(|text| parse_timestamp(text)),
                        );
                        self.user_seeking = response.dragged();
                        if response.drag_stopped() {
                            self.user_seeking = false;
                            self.seek();
                        }
                        let time_response = ui.add_enabled(
                            self.mpv.is_some(),
                            TextEdit::singleline(&mut self.seek_input_buffer)
                                .desired_width(72.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        if time_response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            if let Some(seconds) = parse_timestamp(&self.seek_input_buffer) {
                                self.seek_position = seconds.max(0.0);
                                self.seek();
                            } else {
                                self.status =
                                    "Invalid seek time. Use MM:SS or HH:MM:SS.".to_owned();
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("OPAC").monospace());
                        ui.add(Slider::new(&mut self.opacity, 0.45..=1.0).show_value(true));
                    });

                    if !self.now_playing_title.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("Now: {}", self.now_playing_title))
                                .monospace()
                                .color(Color32::from_rgb(210, 215, 225)),
                        );
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new("Chapters").strong());
                    ScrollArea::vertical()
                        .id_salt("chapters_scroll")
                        .max_height(150.0)
                        .show(ui, |ui| {
                            if self.chapters.is_empty() {
                                ui.label(RichText::new("No chapter markers found.").small());
                            }

                            for index in 0..self.chapters.len() {
                                let chapter = &self.chapters[index];
                                let label = format!(
                                    "{}  {}",
                                    format_duration(chapter.start_time),
                                    chapter.title
                                );
                                if ui.selectable_label(false, label).clicked() {
                                    self.seek_chapter(chapter.start_time);
                                }
                            }
                        });
                });

                columns[1].vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.active_tab == 0, "Play Queue")
                            .clicked()
                        {
                            self.active_tab = 0;
                        }
                        ui.label("|");
                        if ui
                            .selectable_label(self.active_tab == 1, "History")
                            .clicked()
                        {
                            self.active_tab = 1;
                        }
                    });
                    ui.add_space(4.0);

                    if self.active_tab == 0 {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Queue").strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Clear").clicked() {
                                        self.stop();
                                        self.playlist.clear();
                                        self.selected_track = None;
                                        self.chapters.clear();
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);

                        ScrollArea::vertical()
                            .id_salt("play_queue_scroll")
                            .max_height(250.0)
                            .show(ui, |ui| {
                                if self.playlist.is_empty() {
                                    ui.label(
                                        RichText::new("Queue is empty. Enter URL to Add/Load.")
                                            .small(),
                                    );
                                }

                                let mut to_remove = None;
                                let mut to_move_up = None;
                                let mut to_move_down = None;

                                for index in 0..self.playlist.len() {
                                    let item = self.playlist[index].clone();
                                    let selected = self.selected_track == Some(index);
                                    let duration = item
                                        .duration
                                        .map(format_duration)
                                        .unwrap_or_else(|| "--:--".to_owned());

                                    ui.horizontal(|ui| {
                                        let title_text =
                                            format!("{:02}. {}", index + 1, item.title);
                                        let resp = ui.selectable_label(
                                            selected,
                                            RichText::new(title_text).color(if selected {
                                                Color32::from_rgb(245, 190, 68)
                                            } else {
                                                Color32::from_rgb(200, 205, 215)
                                            }),
                                        );

                                        if resp.clicked() {
                                            self.play_track(index);
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(duration)
                                                        .small()
                                                        .color(Color32::from_rgb(120, 122, 130)),
                                                );

                                                if ui.small_button("❌").clicked() {
                                                    to_remove = Some(index);
                                                }
                                                if index > 0 && ui.small_button("🔼").clicked() {
                                                    to_move_up = Some(index);
                                                }
                                                if index < self.playlist.len() - 1
                                                    && ui.small_button("🔽").clicked()
                                                {
                                                    to_move_down = Some(index);
                                                }
                                            },
                                        );
                                    });
                                }

                                if let Some(idx) = to_remove {
                                    self.playlist.remove(idx);
                                    if self.selected_track == Some(idx) {
                                        self.stop();
                                        self.selected_track = if self.playlist.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        };
                                    } else if let Some(sel) = self.selected_track {
                                        if sel > idx {
                                            self.selected_track = Some(sel - 1);
                                        }
                                    }
                                }
                                if let Some(idx) = to_move_up {
                                    self.playlist.swap(idx, idx - 1);
                                    if self.selected_track == Some(idx) {
                                        self.selected_track = Some(idx - 1);
                                    } else if self.selected_track == Some(idx - 1) {
                                        self.selected_track = Some(idx);
                                    }
                                }
                                if let Some(idx) = to_move_down {
                                    self.playlist.swap(idx, idx + 1);
                                    if self.selected_track == Some(idx) {
                                        self.selected_track = Some(idx + 1);
                                    } else if self.selected_track == Some(idx + 1) {
                                        self.selected_track = Some(idx);
                                    }
                                }
                            });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Listening History").strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Clear").clicked() {
                                        self.history.clear();
                                        let _ = save_history(&self.history);
                                    }
                                },
                            );
                        });
                        ScrollArea::vertical()
                            .id_salt("history_scroll")
                            .max_height(250.0)
                            .show(ui, |ui| {
                                if self.history.is_empty() {
                                    ui.label(RichText::new("No history items recorded.").small());
                                }

                                for index in 0..self.history.len() {
                                    let item = self.history[index].clone();
                                    let duration = item
                                        .duration
                                        .map(format_duration)
                                        .unwrap_or_else(|| "--:--".to_owned());
                                    let saved_at = format_duration(item.last_position);

                                    ui.horizontal(|ui| {
                                        let label = format!("{:02}. {}", index + 1, item.title);
                                        let resp = ui.selectable_label(
                                            false,
                                            RichText::new(label)
                                                .color(Color32::from_rgb(200, 205, 215)),
                                        );

                                        if resp.clicked() {
                                            let url = item.url.clone();
                                            self.url = url.clone();
                                            self.original_input_url = url.clone();

                                            self.seek_position = item.last_position;
                                            self.seek_input_buffer =
                                                format_duration(item.last_position);
                                            self.time_pos = item.last_position;

                                            let queue_idx =
                                                self.playlist.iter().position(|q| q.url == url);
                                            if let Some(q_idx) = queue_idx {
                                                self.play_track(q_idx);
                                            } else {
                                                let new_idx = self.playlist.len();
                                                self.playlist.push(PlaylistItem {
                                                    title: item.title.clone(),
                                                    url: url.clone(),
                                                    duration: item.duration,
                                                });
                                                self.play_track(new_idx);
                                            }
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{}/{}",
                                                        saved_at, duration
                                                    ))
                                                    .small()
                                                    .color(Color32::from_rgb(120, 122, 130)),
                                                );
                                            },
                                        );
                                    });
                                }
                            });
                    }
                });
            });

            if self.show_debug {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Debug").strong());
                    if ui.button("Export Logs").clicked() {
                        self.export_debug_logs();
                    }
                });
                ScrollArea::vertical()
                    .id_salt("debug_scroll")
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for line in &self.debug_logs {
                            ui.label(RichText::new(line).small().monospace());
                        }
                    });
            }

            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("MPV: {MPV_PATH}"))
                    .small()
                    .color(Color32::from_rgb(140, 142, 150)),
            );
        });
    }
}

fn fetch_playlist_items(url: &str) -> (Vec<PlaylistItem>, Vec<String>) {
    let mut logs = vec![format!("Fetching metadata with yt-dlp: {url}")];
    let mut playlist = Vec::new();

    let mut command = hidden_command("yt-dlp");
    let output = command
        .args(["--flat-playlist", "--dump-json", "--no-warnings", url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for (index, line) in stdout.lines().enumerate() {
                let Ok(item) = serde_json::from_str::<YtdlpItem>(line) else {
                    continue;
                };
                let item_url = item
                    .webpage_url
                    .or(item.url)
                    .unwrap_or_else(|| url.to_owned());
                playlist.push(PlaylistItem {
                    title: item
                        .title
                        .unwrap_or_else(|| format!("Track {}", index.saturating_add(1))),
                    url: item_url,
                    duration: item.duration,
                });
            }
            logs.push(format!("yt-dlp returned {} item(s).", playlist.len()));
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            logs.push(format!("yt-dlp failed: {stderr}"));
        }
        Err(err) => {
            logs.push(format!("yt-dlp unavailable: {err}"));
        }
    }

    if playlist.is_empty() {
        playlist.push(PlaylistItem {
            title: url.to_owned(),
            url: url.to_owned(),
            duration: None,
        });
    }

    (playlist, logs)
}

fn fetch_chapter_items(url: &str) -> (Vec<Chapter>, Vec<String>) {
    let mut logs = vec![format!("Fetching chapter metadata: {url}")];
    let mut chapters = Vec::new();

    let mut command = hidden_command("yt-dlp");
    let output = command
        .args(["--dump-single-json", "--no-playlist", "--no-warnings", url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match output {
        Ok(output) if output.status.success() => {
            if let Ok(item) = serde_json::from_slice::<YtdlpItem>(&output.stdout) {
                if let Some(items) = item.chapters {
                    for (index, chapter) in items.into_iter().enumerate() {
                        if let Some(start_time) = chapter.start_time {
                            chapters.push(Chapter {
                                title: chapter
                                    .title
                                    .unwrap_or_else(|| format!("Chapter {}", index + 1)),
                                start_time,
                            });
                        }
                    }
                }
            }
            logs.push(format!("yt-dlp returned {} chapter(s).", chapters.len()));
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            logs.push(format!("chapter metadata failed: {stderr}"));
        }
        Err(err) => {
            logs.push(format!("chapter metadata unavailable: {err}"));
        }
    }

    (chapters, logs)
}

fn state_file_path() -> PathBuf {
    PathBuf::from(STATE_FILE)
}

fn load_session_state() -> Option<SessionState> {
    let path = state_file_path();
    let contents = fs::read_to_string(path).ok()?;
    if let Ok(state) = serde_json::from_str::<SessionState>(&contents) {
        return Some(state);
    }

    serde_json::from_str::<LegacySessionState>(&contents)
        .ok()
        .map(|state| SessionState {
            original_input_url: state.current_url,
            current_track_index: state.active_playlist_index.unwrap_or(0),
            playback_position_seconds: state.playback_position_seconds,
        })
}

fn save_session_state(state: &SessionState) -> Result<(), String> {
    let path = state_file_path();
    let contents = serde_json::to_string_pretty(state)
        .map_err(|err| format!("state serialization failed: {err}"))?;
    fs::write(path, contents).map_err(|err| format!("state write failed: {err}"))
}

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[derive(Deserialize, Serialize)]
struct IpcResponse {
    data: Option<serde_json::Value>,
    error: Option<String>,
}

fn send_ipc_command(
    ipc_path: &str,
    command: serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    let payload = json!({ "command": command }).to_string();
    let deadline = Instant::now() + Duration::from_millis(900);

    loop {
        match OpenOptions::new().read(true).write(true).open(ipc_path) {
            Ok(mut pipe) => {
                writeln!(pipe, "{payload}")
                    .map_err(|err| format!("MPV IPC command failed: {err}"))?;
                pipe.flush()
                    .map_err(|err| format!("MPV IPC flush failed: {err}"))?;

                let mut response = String::new();
                let mut reader = BufReader::new(pipe);
                reader
                    .read_line(&mut response)
                    .map_err(|err| format!("MPV IPC response failed: {err}"))?;
                if response.trim().is_empty() {
                    return Ok(None);
                }

                let response: IpcResponse = serde_json::from_str(&response)
                    .map_err(|err| format!("MPV IPC response parse failed: {err}"))?;
                if matches!(response.error.as_deref(), Some("success") | None) {
                    return Ok(response.data);
                }
                return Err(format!(
                    "MPV IPC error: {}",
                    response.error.unwrap_or_else(|| "unknown".to_owned())
                ));
            }
            Err(err) if Instant::now() < deadline => {
                // If named pipe is not found, MPV is exited. Return error immediately without retry.
                if err.kind() == std::io::ErrorKind::NotFound {
                    return Err(format!("MPV IPC unavailable (Not Found): {err}"));
                }
                thread::sleep(Duration::from_millis(25));
                let _ = err;
            }
            Err(err) => return Err(format!("MPV IPC unavailable: {err}")),
        }
    }
}

fn get_ipc_property(ipc_path: &str, property: &str) -> Option<serde_json::Value> {
    send_ipc_command(ipc_path, json!(["get_property", property]))
        .ok()
        .flatten()
}

fn poll_mpv_ipc(ipc_path: String, tx: mpsc::Sender<MpvEvent>) {
    loop {
        thread::sleep(Duration::from_millis(200));

        // Fast-path check: Test connection using "pause" property. If the IPC command returns Err,
        // it means the Named Pipe is gone or MPV is closed, so we break the loop immediately.
        let pause_result = send_ipc_command(&ipc_path, json!(["get_property", "pause"]));
        if let Err(err) = pause_result {
            let _ = tx.send(MpvEvent::IpcError(err));
            break;
        }
        let paused = pause_result.ok().flatten().and_then(|v| v.as_bool());

        let time_pos = get_ipc_property(&ipc_path, "time-pos").and_then(|value| value.as_f64());
        let duration = get_ipc_property(&ipc_path, "duration").and_then(|value| value.as_f64());
        let media_title = get_ipc_property(&ipc_path, "media-title")
            .and_then(|value| value.as_str().map(ToOwned::to_owned));
        let playlist_index = get_ipc_property(&ipc_path, "playlist-pos")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);

        if tx
            .send(MpvEvent::State {
                paused,
                time_pos,
                duration,
                playlist_index,
                media_title,
            })
            .is_err()
        {
            break;
        }
    }
}

fn read_mpv_stream<R: std::io::Read + Send + 'static>(stream: R, tx: mpsc::Sender<MpvEvent>) {
    let mut reader = stream;
    let mut byte = [0_u8; 1];
    let mut line = Vec::new();

    loop {
        let Ok(count) = reader.read(&mut byte) else {
            break;
        };
        if count == 0 {
            break;
        }

        if byte[0] == b'\r' || byte[0] == b'\n' {
            let text = String::from_utf8_lossy(&line).trim().to_owned();
            line.clear();
            if !text.is_empty() {
                let _ = tx.send(parse_mpv_line(&text));
            }
        } else {
            line.push(byte[0]);
        }
    }
}

fn parse_mpv_line(line: &str) -> MpvEvent {
    if let Some(rest) = line.strip_prefix("APP_STATE ") {
        return MpvEvent::State {
            paused: read_value(rest, "pause=").and_then(parse_mpv_bool),
            time_pos: read_value(rest, "time=").and_then(|value| value.parse().ok()),
            duration: read_value(rest, "duration=").and_then(|value| value.parse().ok()),
            playlist_index: read_value(rest, "playlist=").and_then(|value| value.parse().ok()),
            media_title: None,
        };
    }

    if let Some(rest) = line.strip_prefix("APP_EVENT ") {
        return MpvEvent::NowPlaying {
            title: read_title(rest),
            playlist_index: read_value(rest, "playlist=").and_then(|value| value.parse().ok()),
        };
    }

    MpvEvent::Log(line.to_owned())
}

fn read_value<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    source
        .split_whitespace()
        .find_map(|part| part.strip_prefix(key))
        .filter(|value| !value.is_empty() && *value != "N/A")
}

fn read_title(source: &str) -> String {
    source
        .split(" playlist=")
        .next()
        .unwrap_or_default()
        .strip_prefix("title=")
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn parse_mpv_bool(value: &str) -> Option<bool> {
    match value {
        "yes" | "true" => Some(true),
        "no" | "false" => Some(false),
        _ => None,
    }
}

fn format_duration(duration: f64) -> String {
    let total = duration.round().max(0.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn parse_timestamp(text: &str) -> Option<f64> {
    let parts = text
        .trim()
        .split(':')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    match parts.as_slice() {
        [seconds] => Some(*seconds),
        [minutes, seconds] => Some(minutes * 60.0 + seconds),
        [hours, minutes, seconds] => Some(hours * 3600.0 + minutes * 60.0 + seconds),
        _ => None,
    }
}

fn install_unicode_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // Load font from file at runtime instead of include_bytes! to avoid
    // embedding ~15 MB into the binary (which stays in RAM permanently).
    let font_path = Path::new(r"C:\Windows\Fonts\LeelawUI.ttf");
    if let Ok(font_bytes) = fs::read(font_path) {
        fonts.font_data.insert(
            "leelawadee_ui_thai".to_owned(),
            FontData::from_owned(font_bytes).into(),
        );

        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "leelawadee_ui_thai".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("leelawadee_ui_thai".to_owned());
    }

    ctx.set_fonts(fonts);
}

fn apply_style(ctx: &egui::Context, opacity: f32) {
    let alpha = (opacity.clamp(0.45, 1.0) * 255.0).round() as u8;
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgba_unmultiplied(12, 14, 18, alpha);
    visuals.window_fill = Color32::from_rgba_unmultiplied(12, 14, 18, alpha);
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(4, 5, 7, alpha);
    visuals.override_text_color = Some(Color32::from_rgb(238, 242, 248));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(238, 242, 248));
    visuals.widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(38, 44, 54, alpha);
    visuals.widgets.hovered.bg_fill = Color32::from_rgba_unmultiplied(58, 68, 82, alpha);
    visuals.widgets.active.bg_fill = Color32::from_rgba_unmultiplied(80, 94, 112, alpha);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(245, 247, 250));
    visuals.selection.bg_fill = Color32::from_rgb(184, 124, 32);
    visuals.hyperlink_color = Color32::from_rgb(115, 190, 255);
    ctx.set_visuals(visuals);
}

fn status_color(state: PlaybackState) -> Color32 {
    match state {
        PlaybackState::Stopped => Color32::from_rgb(180, 184, 190),
        PlaybackState::Loading => Color32::from_rgb(245, 190, 68),
        PlaybackState::Playing => Color32::from_rgb(120, 220, 120),
        PlaybackState::Paused => Color32::from_rgb(120, 190, 245),
        PlaybackState::Error => Color32::from_rgb(245, 90, 90),
    }
}

fn load_history() -> Vec<HistoryItem> {
    let path = Path::new("history.json");
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Vec<HistoryItem>>(&contents).ok())
        .unwrap_or_default()
}

fn save_history(history: &[HistoryItem]) -> Result<(), String> {
    let path = Path::new("history.json");
    let limited_history = if history.len() > 100 {
        &history[0..100]
    } else {
        history
    };
    let contents = serde_json::to_string_pretty(limited_history)
        .map_err(|err| format!("history serialization failed: {err}"))?;
    fs::write(path, contents).map_err(|err| format!("history write failed: {err}"))
}

impl PlayerApp {
    fn update_history_item(
        &mut self,
        url: &str,
        title: &str,
        position: f64,
        duration: Option<f64>,
        save_to_disk: bool,
    ) {
        if url.trim().is_empty() {
            return;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();

        let clean_title = title.trim();
        if let Some(pos) = self.history.iter().position(|item| item.url == url) {
            self.history[pos].last_position = position;
            self.history[pos].last_played = now;
            if duration.is_some() && duration != Some(0.0) {
                self.history[pos].duration = duration;
            }
            if !clean_title.is_empty() && self.history[pos].title != clean_title {
                self.history[pos].title = clean_title.to_owned();
            }
            let item = self.history.remove(pos);
            self.history.insert(0, item);
        } else {
            let item = HistoryItem {
                url: url.to_owned(),
                title: if clean_title.is_empty() {
                    url.to_owned()
                } else {
                    clean_title.to_owned()
                },
                last_position: position,
                duration,
                last_played: now,
            };
            self.history.insert(0, item);
        }

        if save_to_disk {
            let _ = save_history(&self.history);
        }
    }

    fn get_last_position(&self, url: &str) -> Option<f64> {
        self.history
            .iter()
            .find(|item| item.url == url)
            .map(|item| item.last_position)
    }
}

fn extract_time_from_url(url: &str) -> (String, Option<f64>) {
    let mut cleaned_url = url.to_owned();
    let mut extracted_seconds: Option<f64> = None;

    for param in &["t=", "start="] {
        if let Some(pos) = cleaned_url.find(param) {
            let start_val = pos + param.len();
            let end_val = cleaned_url[start_val..]
                .find('&')
                .map(|idx| start_val + idx)
                .unwrap_or(cleaned_url.len());

            let val_str = &cleaned_url[start_val..end_val];
            if let Some(secs) = parse_url_time_string(val_str) {
                extracted_seconds = Some(secs);
            }

            let start_remove = if pos > 0 && cleaned_url.as_bytes()[pos - 1] == b'&' {
                pos - 1
            } else if pos > 0 && cleaned_url.as_bytes()[pos - 1] == b'?' {
                if end_val < cleaned_url.len() && cleaned_url.as_bytes()[end_val] == b'&' {
                    pos
                } else {
                    pos - 1
                }
            } else {
                pos
            };

            let mut end_remove = end_val;
            if start_remove == pos
                && end_val < cleaned_url.len()
                && cleaned_url.as_bytes()[end_val] == b'&'
            {
                end_remove = end_val + 1;
            }

            cleaned_url.replace_range(start_remove..end_remove, "");

            if cleaned_url.contains("?&") {
                cleaned_url = cleaned_url.replace("?&", "?");
            }
            if cleaned_url.ends_with('?') || cleaned_url.ends_with('&') {
                cleaned_url.pop();
            }

            break;
        }
    }

    (cleaned_url, extracted_seconds)
}

fn parse_url_time_string(s: &str) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    let clean_s = s.strip_suffix('s').unwrap_or(&s);
    if let Ok(secs) = clean_s.parse::<f64>() {
        return Some(secs);
    }

    let mut total_secs = 0.0;
    let mut current_num = String::new();
    for c in s.chars() {
        if c.is_digit(10) || c == '.' {
            current_num.push(c);
        } else {
            if let Ok(val) = current_num.parse::<f64>() {
                match c {
                    'h' => total_secs += val * 3600.0,
                    'm' => total_secs += val * 60.0,
                    's' => total_secs += val,
                    _ => {}
                }
            }
            current_num.clear();
        }
    }
    if total_secs > 0.0 {
        Some(total_secs)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Lightweight HTTP API Server (zero external dependencies)
// ---------------------------------------------------------------------------

fn start_api_server(tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        let addr = format!("0.0.0.0:{API_PORT}");
        let listener = match TcpListener::bind(&addr) {
            Ok(l) => {
                eprintln!("[API] Listening on http://{addr}");
                l
            }
            Err(err) => {
                eprintln!("[API] Failed to bind {addr}: {err}");
                return;
            }
        };

        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let peer_addr = stream.peer_addr().ok();
            // Handle each connection in a short-lived thread to avoid blocking
            thread::spawn(move || handle_api_request(stream, peer_addr, &tx));
        }
    });
}

fn start_remote_queue_poller(tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        loop {
            let Some(token) = load_queue_api_token() else {
                thread::sleep(Duration::from_secs(10));
                continue;
            };
            let base_url = std::env::var("LYTB_QUEUE_SERVER_URL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| fs::read_to_string("server-url.txt").ok())
                .map(|value| value.trim().trim_end_matches('/').to_owned())
                .unwrap_or_else(|| "https://player-api.kankrittapon.online".to_owned());
            let device_id = std::env::var("LYTB_DEVICE_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "primary".to_owned());

            let mut command = Command::new("curl.exe");
            command.args([
                "--silent",
                "--show-error",
                "--fail",
                "--max-time",
                "10",
                "-H",
                &format!("Authorization: Bearer {token}"),
                &format!("{base_url}/v1/jobs/next?deviceId={device_id}"),
            ]);
            #[cfg(windows)]
            command.creation_flags(CREATE_NO_WINDOW);

            if let Ok(output) = command.output() {
                if output.status.success() && !output.stdout.is_empty() {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                        let job = value.get("job");
                        let id = job
                            .and_then(|item| item.get("id"))
                            .and_then(|item| item.as_str());
                        let url = job
                            .and_then(|item| item.get("url"))
                            .and_then(|item| item.as_str());
                        if let (Some(id), Some(url)) = (id, url) {
                            if tx.send(url.to_owned()).is_err() {
                                break;
                            }
                            acknowledge_remote_job(&base_url, &token, &device_id, id);
                        }
                    }
                }
            }
            thread::sleep(Duration::from_secs(3));
        }
    });
}

fn acknowledge_remote_job(base_url: &str, token: &str, device_id: &str, job_id: &str) {
    let body = json!({ "deviceId": device_id }).to_string();
    let mut command = Command::new("curl.exe");
    command.args([
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "10",
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        "-H",
        "Content-Type: application/json",
        "--data",
        &body,
        &format!("{base_url}/v1/jobs/{job_id}/complete"),
    ]);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let _ = command.output();
}

fn handle_api_request(
    mut stream: TcpStream,
    peer_addr: Option<SocketAddr>,
    tx: &mpsc::Sender<String>,
) {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => {
            send_http_response(&mut stream, 400, r#"{"error":"empty request"}"#);
            return;
        }
    };

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");

    // CORS preflight
    if first_line.starts_with("OPTIONS ") {
        send_http_cors_preflight(&mut stream);
        return;
    }

    if first_line.starts_with("POST /queue") {
        if !authorize_queue_request(&request, peer_addr) {
            let token_configured = load_queue_api_token().is_some();
            let (status, body) = if token_configured {
                (401, r#"{"error":"unauthorized"}"#)
            } else {
                (
                    503,
                    r#"{"error":"remote queue API is disabled until api-token.txt is configured"}"#,
                )
            };
            send_http_response(&mut stream, status, body);
            return;
        }

        // Extract JSON body after the \r\n\r\n separator
        let body = request
            .find("\r\n\r\n")
            .map(|pos| &request[pos + 4..])
            .unwrap_or("");

        if body.is_empty() {
            send_http_response(
                &mut stream,
                400,
                r#"{"error":"empty body, expected {\"url\":\"...\"}}"#,
            );
            return;
        }

        match serde_json::from_str::<serde_json::Value>(body) {
            Ok(parsed) => {
                if let Some(url) = parsed.get("url").and_then(|v| v.as_str()) {
                    let url = url.trim();
                    if url.is_empty() {
                        send_http_response(&mut stream, 400, r#"{"error":"url field is empty"}"#);
                        return;
                    }
                    if url.len() > 4096
                        || !(url.starts_with("https://") || url.starts_with("http://"))
                    {
                        send_http_response(
                            &mut stream,
                            400,
                            r#"{"error":"url must use http or https and be at most 4096 characters"}"#,
                        );
                        return;
                    }
                    let _ = tx.send(url.to_owned());
                    let resp = json!({"status": "queued", "url": url});
                    send_http_response(&mut stream, 200, &resp.to_string());
                } else {
                    send_http_response(
                        &mut stream,
                        400,
                        r#"{"error":"missing 'url' field in JSON body"}"#,
                    );
                }
            }
            Err(err) => {
                let resp = json!({"error": format!("invalid JSON: {err}")});
                send_http_response(&mut stream, 400, &resp.to_string());
            }
        }
    } else if first_line.starts_with("GET /health") {
        send_http_response(&mut stream, 200, r#"{"status":"ok","app":"LYTBokkChoYx"}"#);
    } else if first_line.starts_with("GET /queue-help") {
        let help = json!({
            "endpoints": {
                "POST /queue": {
                    "description": "Add a URL to the play queue",
                    "body": {"url": "https://youtube.com/watch?v=..."},
                    "remote_auth": "Authorization: Bearer <token from api-token.txt>",
                    "example_curl": format!("curl -X POST http://localhost:{API_PORT}/queue -H \"Content-Type: application/json\" -d '{{\"url\":\"https://youtube.com/watch?v=dQw4w9WgXcQ\"}}'")
                },
                "GET /health": {
                    "description": "Check if the player is running"
                },
                "GET /queue-help": {
                    "description": "Show this help message"
                }
            }
        });
        send_http_response(
            &mut stream,
            200,
            &serde_json::to_string_pretty(&help).unwrap_or_default(),
        );
    } else {
        send_http_response(
            &mut stream,
            404,
            r#"{"error":"not found","hint":"try GET /queue-help"}"#,
        );
    }
}

fn load_queue_api_token() -> Option<String> {
    if let Ok(value) = std::env::var("LYTB_QUEUE_API_TOKEN") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            return Some(value);
        }
    }
    fs::read_to_string("api-token.txt")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn authorize_queue_request(request: &str, peer_addr: Option<SocketAddr>) -> bool {
    if peer_addr.is_some_and(|addr| addr.ip().is_loopback()) {
        return true;
    }
    let Some(expected) = load_queue_api_token() else {
        return false;
    };
    let supplied = request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization").then(|| {
            value
                .trim()
                .strip_prefix("Bearer ")
                .unwrap_or("")
                .to_owned()
        })
    });
    supplied.is_some_and(|value| constant_time_equal(value.as_bytes(), expected.as_bytes()))
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn send_http_response(stream: &mut TcpStream, status_code: u16, body: &str) {
    let status_text = match status_code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status_code} {status_text}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn send_http_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 204 No Content\r\n\
        Access-Control-Allow-Origin: *\r\n\
        Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
        Access-Control-Allow-Headers: Content-Type\r\n\
        Access-Control-Max-Age: 86400\r\n\
        Connection: close\r\n\
        \r\n";
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
