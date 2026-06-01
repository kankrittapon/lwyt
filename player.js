const { spawn } = require('child_process');
const readline = require('readline');

// ==========================================
// 1. ตั้งค่าการเล่น (ตั้งค่า URL ตรงนี้)
// ==========================================
const playlistUrl = "https://www.youtube.com/watch?v=liTj2cga-X8&list=PLTubEPwWLaT7_rDszOkDaj57rF02u3SZu"; 

// ==========================================
// 2. ตั้งค่า Arguments เพื่อรีดการใช้ RAM ขั้นสุด
// ==========================================
const mpvArgs = [
    '--no-video',                 // ปิดภาพเด็ดขาด
    '--ytdl-format=bestaudio',    // โหลดเฉพาะเสียงที่ดีที่สุด
    '--vo=null',                  // ปิด Video Output อัตโนมัติ
    '--playlist-start=1',         // เริ่มที่คลิปแรก (แก้เป็นลำดับคลิปอื่นได้)
    playlistUrl
];

console.log("🚀 Starting Lightweight Audiobook Player...");
console.log("⏳ กำลังโหลดข้อมูล Playlist กรุณารอสักครู่...\n");

// ==========================================
// 3. รัน mpv แบบระบุตำแหน่งไฟล์โดยตรง
// ==========================================
// ใส่ Path เต็มๆ ที่คุณหาเจอ (อย่าลืมใส่ \\mpv.exe ต่อท้าย)
const mpvPath = 'C:\\Program Files\\MPV Player\\mpv.exe';

// รันโปรแกรมจาก Path ที่ระบุ
const mpvProcess = spawn(mpvPath, mpvArgs);

// ==========================================
// 4. จัดการข้อความที่ตอบกลับมาจาก mpv (Log)
// ==========================================
mpvProcess.stdout.on('data', (data) => {
    console.log(`[MPV Log]: ${data.toString().trim()}`);
});

mpvProcess.stderr.on('data', (data) => {
    console.error(`[MPV Error]: ${data.toString().trim()}`);
});

mpvProcess.on('close', (code) => {
    console.log(`\n✅ Player exited (Code: ${code})`);
    process.exit();
});

// ==========================================
// 5. ระบบควบคุมผ่านคีย์บอร์ด (Controller)
// ==========================================
readline.emitKeypressEvents(process.stdin);
if (process.stdin.isTTY) {
    process.stdin.setRawMode(true);
}

console.log("--- 🎛️ Controls ---");
console.log("[ n ] Next Track  |  [ p ] Prev Track  |  [ Space ] Pause/Play  |  [ q ] Quit\n");

process.stdin.on('keypress', (str, key) => {
    if (key.name === 'q' || (key.ctrl && key.name === 'c')) {
        console.log("\n🛑 Exiting player...");
        mpvProcess.kill();
        process.exit();
    } 
    else if (key.name === 'n') {
        mpvProcess.stdin.write('playlist-next\n');
        console.log("⏭️ Skipping to next track...");
    } 
    else if (key.name === 'p') {
        mpvProcess.stdin.write('playlist-prev\n');
        console.log("⏮️ Going to previous track...");
    } 
    else if (key.name === 'space') {
        mpvProcess.stdin.write('cycle pause\n'); 
        console.log("⏯️ Pause / Play toggled...");
    }
});