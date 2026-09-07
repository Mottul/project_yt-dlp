// Verhindert ein zusaetzliches Konsolenfenster unter Windows im Release-Build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mottul_video_loader_lib::run()
}
