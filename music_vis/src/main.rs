mod app;
mod audio;
mod graphics;
mod cwt_file;

use std::path::PathBuf;
use winit::event_loop::EventLoop;
use app::App;
use cwt_file::CwtFile;

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: music_vis <audio.flac|mp3|wav|ogg> <audio.cwt>");
        std::process::exit(1);
    }
    let audio_path = PathBuf::from(&args[1]);
    let cwt_path   = PathBuf::from(&args[2]);

    let cwt = CwtFile::read(&cwt_path).expect("Failed to read CWT file");

    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new(audio_path, cwt);

    event_loop.run_app(&mut app).unwrap();
}
