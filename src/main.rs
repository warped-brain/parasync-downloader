use bytes::Bytes;
use parasync_downloader::async_downloader::{async_file_aggregator, AsyncDownloader};
use std::{
    thread::{self, JoinHandle, Thread},
    time::Instant,
};
use tokio::runtime;

fn main() {
    let url =
        "https://static.videezy.com/system/protected/files/000/002/421/heavy_rain_on_tree_branches.mp4".to_string();
    let out_file = "a.mp4".to_string();
    println!("Async Download Time: ");
    let i = Instant::now();
    let (mut sx, mut rx) = std::sync::mpsc::channel::<(usize, Bytes)>();
    println!("Started Thread");
    let h = std::thread::spawn(move || {
        async_file_aggregator(rx, out_file);
    });
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(6)
        .enable_all()
        .build()
        .unwrap();
    let ad = AsyncDownloader::new(rt, sx);
    ad.async_download_url_chunks(&url, 100_000_u32);

    h.join();
    println!("Finished");
    // download_url_chunks(&url.to_string(), &out_file.to_string(), 100000_u32);
    println!("Time: {:?}", i.elapsed());

    // println!("Sync Download Time: ");
    //
    // let i = Instant::now();
    // parasync_downloader::sync_downloader::download_url(&url, &out_file);
    // println!("Finished");
    // // download_url_chunks(&url.to_string(), &out_file.to_string(), 100000_u32);
    // println!("Time: {:?}", i.elapsed());
}
