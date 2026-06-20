# Lightweight Audio Player (MINIAMP)

A highly optimized, ultra-lightweight audio player written in Rust using the `eframe` (egui) GUI framework. It controls `mpv` player in the background via Windows Named Pipe IPC to stream audio from YouTube, online streams, or local files with a strict focus on minimizing PC RAM usage.

> **RAM Footprint:** ~25MB (GUI) + MPV overhead (typically under 40-50MB total).

---

## ✨ Features

- **Winamp-Inspired Minimalist UI:** Classic styling with transparency support.
- **Smart Play Queue Manager:** Add URLs or playlists to the queue seamlessly in the background without interrupting current playback. You can reorder items (🔼/🔽), remove them (❌), or clear the queue.
- **Auto-Advance Playback:** Automatically transitions to the next track in the queue when the current song finishes.
- **History Tracker & State Persistence:** Tracks listening history (up to 100 items saved to `history.json` to keep RAM under 2MB). It remembers the last playback position for each unique URL and automatically resumes from where you left off, even if the application is reopened.
- **Time Parsing from URL:** Paste YouTube links containing timestamp parameters (e.g., `&t=1h30m5s` or `?t=4500s`). The player strips it cleanly to prevent playback conflicts and resumes play at the correct timestamp.
- **Robust Windows IPC & Auto-Reconnection:** Uses fast-path named pipe checking to avoid zombie processes. If the IPC named pipe is disconnected or MPV crashes unexpectedly, it automatically reconnects and resumes playback from the last known time position.

---

## 🛠️ Prerequisites

To run this application, you must have the following installed on your Windows system:

1. **Rust (Cargo):** To compile and run the project.
2. **MPV Player:** Installed at `C:\Program Files\MPV Player\mpv.exe`.
3. **yt-dlp:** Added to your system Environment Variables (PATH) so that metadata and streams can be resolved.

---

## 📦 How to Build & Run Locally

```bash
# Run in development mode
cargo run

# Build release executable
cargo build --release
```

The compiled release binary will be available at `target/release/lightweight_audio_player.exe`.

---

## 🚀 Deployment & Installation (.MSI / .EXE Setup)

The project includes setup configurations to easily package and distribute the player without manual configuration:

### 1. GitHub Actions CI/CD (Recommended)
This repository is configured with a GitHub Actions workflow (`.github/workflows/release.yml`). It automatically compiles the Rust binary, generates the `.msi` installer, and publishes them as a GitHub Release:
- Just push a new version tag (e.g., `v0.1.0`):
  ```bash
  git tag v0.1.0
  git push origin master --tags
  ```

### 2. WiX Toolset Installer (.MSI)
A WiX v4/v5 script ([installer.wxs](installer.wxs)) is available in the root directory to generate standard MSI packages.
```bash
wix build installer.wxs -o LightweightAudioPlayer.msi
```

### 3. Inno Setup Installer (.EXE with dependency checks)
An Inno Setup script ([installer.iss](installer.iss)) is provided. It compiles a standard setup file and includes automated checks at the end of the installation to verify if `mpv.exe` and `yt-dlp` are installed in the user's system PATH.

---

## 🎛️ Controls

- **Load:** Loads the URL specified in the address bar, clearing the current queue and playing immediately.
- **+ Queue:** Appends the specified URL/Playlist metadata to the end of the current Play Queue in the background.
- **Play/Pause / Stop:** Core playback controls.
- **Prev / Next:** Jump to the previous or next track in the queue.
- **VOL Slider:** Adjust volume (synchronized with mpv).
- **SEEK Slider:** Drag to seek through the current track, or enter a timestamp format (e.g., `MM:SS` or `HH:MM:SS`) in the text box and press Enter.
- **OPAC Slider:** Adjust window opacity (45% to 100%).

---

## 🇹🇭 คู่มือการใช้งานภาษาไทย

**MINIAMP** คือเครื่องเล่นเพลงขนาดเล็กพิเศษ เขียนด้วยภาษา Rust โดยจำกัดการใช้ RAM ขั้นสุด

### การติดตั้งและใช้งาน
1. ติดตั้งโปรแกรม **MPV Player** ไว้ที่พาธ: `C:\Program Files\MPV Player\mpv.exe`
2. ติดตั้ง **yt-dlp** และทำการเพิ่มลงใน PATH ของ Windows
3. สั่งรันด้วยคำสั่ง `cargo run` หรือแพ็คตัวติดตั้งด้วย Inno Setup / WiX Toolset
4. หากคุณทำการอัปโหลดโค้ดขึ้น GitHub และสร้าง Tag ระบบ GitHub Actions จะทำหน้าที่คอมไพล์และสร้างไฟล์ติดตั้งแบบ `.msi` ส่งออกมาให้ดาวน์โหลดโดยอัตโนมัติ!
