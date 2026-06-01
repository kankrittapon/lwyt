"Develop a highly lightweight audio player application with a strict constraint on minimizing PC RAM usage. The application should feature a classic Winamp-style user interface. Key technical requirements include:

1.Ultra-Low Memory Footprint: The architecture must be heavily optimized to consume the absolute minimum amount of RAM possible. (Recommendation: Utilize Rust for the core logic and memory safety, paired with a lightweight framework like Tauri instead of Electron, or a native Rust GUI framework like egui or slint to avoid heavy web engine overhead).

2.Dynamic Source Management: Implement a built-in feature that allows users to directly input, edit, and load audio file URLs or streaming links within the application interface.

3.Integrated Playback Controls: Include comprehensive built-in media controls (Play, Pause, Stop, Volume adjustment, and seeking) seamlessly integrated into the Winamp-inspired UI.

4.Tech Stack: Please design the solution using Rust for the backend/system-level optimization, and optionally Node.js/React for a minimal frontend if using a lightweight webview approach like Tauri."

