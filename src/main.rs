use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use parasync_downloader::async_downloader::{AsyncDownloader, DownloadTask, ProgressEnum};
use std::{
    fs::OpenOptions,
    sync::Arc,
    time::{Duration, Instant},
};

use std::fmt::Write;

use ::humantime::format_duration;

fn main() {
    // std::env::set_var("RUST_BACKTRACE", "1");
    // let url = "https://mirror.alwyzon.net/archlinux/iso/2024.02.01/archlinux-2024.02.01-x86_64.iso"
    //     .to_string();
    let url = std::env::args().nth(1).unwrap();
    // let out_file = "ubu.iso".to_string();
    let out_file = std::env::args().nth(2).unwrap();
    let out_file = std::path::Path::new(&out_file);
    if out_file.exists() {
        std::fs::remove_file(out_file);
    };
    let out_file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(out_file)
        .unwrap();
    // println!("Async Download Time: ");
    let i = Instant::now();
    let (task_sx, task_rx) = tokio::sync::mpsc::channel::<DownloadTask>(5);
    let (progress_sx, progress_rx) =
        tokio::sync::mpsc::channel::<parasync_downloader::async_downloader::ProgressEnum>(10);
    let downloadtask = DownloadTask {
        chunk_size: 20_000_000,
        url: Arc::new(url),
        multi_connection: 3,
        chunk_config: None,
        out_file: out_file,
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .build()
        .expect("Tokio Runtime");
    task_sx.blocking_send(downloadtask);
    // println!("Started Thread");

    std::thread::spawn(|| {
        let rtc = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Tokio Runtime");

        rtc.block_on(update_prog(progress_rx));
    });

    let mut downloader = AsyncDownloader::new(&rt, task_rx, progress_sx.clone(), task_sx);
    // println!("Finished");
    rt.block_on(downloader.get_tasks());

    // download_url_chunks(&url.to_string(), &out_file.to_string(), 100000_u32);
    println!("Donwload finished in: {:?}", i.elapsed());
    // println!("Sync Download Time: ");
    //
    // let i = Instant::now();
    // parasync_downloader::sync_downloader::download_url(&url, &out_file);
    // println!("Finished");
    // // download_url_chunks(&url.to_string(), &out_file.to_string(), 100000_u32);
    // println!("Time: {:?}", i.elapsed());
    // tokio::join!();
}

pub fn mb_to_kb(size: u64) -> u64 {
    size * 1024 * 1024
}

pub async fn update_prog(mut progress_rx: tokio::sync::mpsc::Receiver<ProgressEnum>) {
    let mut pb_init = false;
    let mut pb = ProgressBar::new(100);
    let mut downloaded = 0.0;
    let mut last_downloaded = 0.0;
    let mut e_tick = tokio::time::Instant::now();
    let mut speed = 0.0;
    // println!("Called Update");
    while !pb_init {
        // println!("Received Content");
        let prog = progress_rx.recv().await.unwrap();
        println!("{:?}", &prog);
        match prog {
            parasync_downloader::async_downloader::ProgressEnum::content_length(l) => {
                pb = ProgressBar::new(l);
                pb_init = true;
            }
            parasync_downloader::async_downloader::ProgressEnum::progres(_) => {}
        }
    }
    while let Some(prog) = progress_rx.recv().await {
        // println!("Recied Prog");
        match prog {
            parasync_downloader::async_downloader::ProgressEnum::content_length(_) => {}
            parasync_downloader::async_downloader::ProgressEnum::progres(n) => {
                pb.enable_steady_tick(Duration::from_secs(1));
                pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta}@{mbps})")
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| write!(w, "{}", format_duration(Duration::from_secs(state.eta().as_secs()))).unwrap())
                    .with_key("mbps", move  |_state: &ProgressState, w: &mut dyn Write| {write!(w, "{:.2}MBps", speed).unwrap();})
        .progress_chars("#>-"));
                if e_tick.elapsed() >= Duration::from_secs(1) {
                    e_tick = tokio::time::Instant::now();
                    speed = last_downloaded / 1048576.0;

                    last_downloaded = 0.0;
                };
                last_downloaded += n as f64;
                downloaded += n as f64;
                pb.set_position(downloaded as u64);
            }
        }
    }
    pb.finish();
}

// pub async fn initialise_downloader(
//     task_rx: tokio::sync::mpsc::Receiver<DownloadTask>,
//     progress_sx: std::sync::mpsc::Sender<u64>,
//     task_sx: tokio::sync::mpsc::Sender<DownloadTask>,
// ) {
// }
