# Lightweight Audio Player (MINIAMP)

A highly optimized, ultra-lightweight audio player written in Rust using the `eframe` (egui) GUI framework. It controls `mpv` player in the background via Windows Named Pipe IPC to stream audio from YouTube, online streams, or local files with a strict focus on minimizing PC RAM usage.

> **RAM Footprint:** ~25MB (GUI) + MPV overhead (typically under 40-50MB total).

---

## ✨ Features

- **Winamp-Inspired Minimalist UI:** Classic styling with transparency support.
- **Smart Play Queue Manager:** Add URLs or playlists to the queue seamlessly in the background without interrupting current playback. You can reorder items (🔼/🔽), remove them (❌), or clear the queue.
- **Auto-Advance Playback:** Automatically transitions to the next track in the queue when the current song finishes.
- **History Tracker & Auto-Resume:** Tracks your listening history (saves up to 100 items to `history.json` to keep RAM under 2MB). It remembers the last playback position for each unique URL and automatically resumes from where you left off.
- **Time Parsing from URL:** Paste YouTube links containing timestamp parameters (e.g., `&t=1h30m5s` or `?t=4500s`). The player strips it cleanly to prevent playback conflicts and resumes play at the correct timestamp.
- **Robust Windows IPC Handling:** Uses fast-path exit named pipe configurations to prevent background zombie threads and program freezes upon reopening.

---

## 🛠️ Prerequisites

To run this application, you must have the following installed on your Windows system:

1. **Rust (Cargo):** To compile and run the project.
2. **MPV Player:** Installed at `C:\Program Files\MPV Player\mpv.exe`.
3. **yt-dlp:** Added to your system Environment Variables (PATH) so that metadata and streams can be resolved.

---

## 🚀 How to Run

Clone the repository and run the following command in your terminal:

```bash
# Run in development mode
cargo run

# Build release executable
cargo build --release
```

The compiled release binary will be available at `target/release/lightweight_audio_player.exe`.

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

### การติดตั้งและสิ่งจำเป็น
1. ติดตั้งภาษา **Rust**
2. ติดตั้งโปรแกรม **MPV Player** ไว้ที่พาธ: `C:\Program Files\MPV Player\mpv.exe`
3. ติดตั้ง **yt-dlp** และทำการเพิ่มลงใน PATH ของ Windows เพื่อดึงข้อมูลสตรีมและรายชื่อเพลง

### การสั่งรันโปรแกรม
เปิด Git Bash หรือ Terminal ในโฟลเดอร์โปรเจกต์แล้วรัน:
```bash
# สั่งรันโปรแกรมทันที
cargo run

# สั่งสร้างไฟล์โปรแกรมสำหรับนำไปใช้งาน (.exe)
cargo build --release
```

### ฟังก์ชันพิเศษของระบบ
*   **ระบบจดจำตำแหน่งประวัติ:** ตัวโปรแกรมจะบันทึกประวัติการฟังไว้ในไฟล์ `history.json` (สูงสุด 100 รายการเพื่อไม่ให้บวมแรม) และจะจดจำเวลาเล่นค้างล่าสุดของแต่ละ URL เมื่อคุณเปิดเพลงเดิมอีกครั้ง มันจะเล่นต่อจากจุดเดิมให้อัตโนมัติ
*   **ระบบคิวเล่นเพลง (Play Queue):** สามารถป้อนลิงก์แล้วกด `+ Queue` เพื่อโหลดรายชื่อเพลงมาต่อท้ายคิวได้เรื่อยๆ โดยระบบจะสตรีมเพลงถัดไปให้อัตโนมัติเมื่อเพลงก่อนหน้าจบลง
