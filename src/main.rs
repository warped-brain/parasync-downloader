use bytes::Bytes;
use parasync_downloader::{
    async_downloader::{AsyncDownloader, DownloadTask},
    sync_downloader::download_url,
};
use std::{
    fs::OpenOptions,
    sync::Arc,
    thread::{self, JoinHandle, Thread},
    time::Instant,
};
use tokio::runtime;

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1");
    let url = "https://getsamplefiles.com/download/mp4/sample-3.mp4".to_string();
    let out_file = "a.mp4".to_string();
    let out_file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(out_file)
        .unwrap();
    println!("Async Download Time: ");
    let i = Instant::now();
    let (mut task_sx, mut task_rx) = tokio::sync::mpsc::channel::<DownloadTask>(5);
    let (mut progress_sx, mut progress_rx) = std::sync::mpsc::channel::<u64>();
    let downloadtask = DownloadTask {
        chunk_size: 1_000_000,
        url: Arc::new(url),
        multi_connection: 10,
        chunk_config: None,
        out_file: out_file,
    };
    task_sx.blocking_send(downloadtask);

    println!("Started Thread");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio Runtime");
    let task_sx_1 = task_sx.clone();

    let mut downloader = AsyncDownloader::new(&rt, task_rx, progress_sx.clone(), task_sx_1);
    println!("Finished");
    rt.block_on(downloader.get_tasks());
    // download_url_chunks(&url.to_string(), &out_file.to_string(), 100000_u32);
    println!("Time: {:?}", i.elapsed());
    // println!("Sync Download Time: ");
    //
    // let i = Instant::now();
    // parasync_downloader::sync_downloader::download_url(&url, &out_file);
    // println!("Finished");
    // // download_url_chunks(&url.to_string(), &out_file.to_string(), 100000_u32);
    // println!("Time: {:?}", i.elapsed());
    // tokio::join!();
}

// pub async fn initialise_downloader(
//     task_rx: tokio::sync::mpsc::Receiver<DownloadTask>,
//     progress_sx: std::sync::mpsc::Sender<u64>,
//     task_sx: tokio::sync::mpsc::Sender<DownloadTask>,
// ) {
// }
