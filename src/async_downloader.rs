use bytes::Bytes;
use reqwest::header::{HeaderValue, CONTENT_LENGTH, RANGE};
use reqwest::{Response, StatusCode};
use std::borrow::Borrow;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::FileExt;
use std::sync::Arc;
use std::usize;
use std::{fs::OpenOptions, io::Write};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::{Mutex, Semaphore};
#[derive(Debug)]
pub enum AsyncErrors {
    InvalidChunkSize,
    UnreadableBytes,
    ReqFetchError,
    RangesUnsupported,
    ParseInt,
    UnexpectedServerResponse,
}
// Rust Cookbook
#[derive(Debug)]
struct PartialRangeIter {
    start: u64,
    end: u64,
    chunk_size: u32,
}

impl PartialRangeIter {
    pub fn new(start: u64, end: u64, chunk_size: u32) -> Result<Self, AsyncErrors> {
        if chunk_size == 0 {
            return Err(AsyncErrors::InvalidChunkSize);
        }
        Ok(PartialRangeIter {
            start,
            end,
            chunk_size,
        })
    }
}

impl Iterator for PartialRangeIter {
    type Item = (u64, u64);
    fn next(&mut self) -> Option<(u64, u64)> {
        if self.start > self.end {
            None
        } else {
            let prev_start = self.start;
            self.start += std::cmp::min(self.chunk_size as u64, self.end - self.start + 1);
            Some((prev_start, self.start - 1))
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ChunkConfig {
    pub bytes_range: [u64; 2],
    pub i: usize,
}
impl ChunkConfig {
    pub fn chunk_size(&self) -> u64 {
        self.bytes_range[1] - self.bytes_range[0] + 1
    }
    pub fn header_value(&self) -> String {
        format!("bytes={}-{}", self.bytes_range[0], self.bytes_range[1])
    }
}

#[derive(Debug)]
pub struct DownloadTask {
    pub chunk_size: usize,
    pub url: Arc<String>,
    pub multi_connection: usize,
    pub chunk_config: Option<ChunkConfig>,
    pub out_file: File,
}
pub struct AsyncDownloader<'a> {
    rt: &'a Runtime,
    task_rx: tokio::sync::mpsc::Receiver<DownloadTask>,
    progress_channel: std::sync::mpsc::Sender<u64>,
    failed_sender: tokio::sync::mpsc::Sender<DownloadTask>,
}
impl<'a> AsyncDownloader<'a> {
    pub fn new(
        rt: &'a tokio::runtime::Runtime,
        task_rx: tokio::sync::mpsc::Receiver<DownloadTask>,
        progress_channel: std::sync::mpsc::Sender<u64>,
        failed_sender: tokio::sync::mpsc::Sender<DownloadTask>,
    ) -> Self {
        Self {
            rt,
            task_rx,
            failed_sender,
            progress_channel,
        }
    }
    pub async fn get_tasks(&mut self) {
        println!("Waitint for tasks");
        while let Some(task) = self.task_rx.recv().await {
            let task = Arc::new(task);
            println!("received task {:?}", *task);
            self.async_downloadtask_handler(task.clone()).await;
        }
    }
    pub async fn async_downloadtask_handler(
        &mut self,
        task: Arc<DownloadTask>,
    ) -> Result<(), AsyncErrors> {
        let chunk_download_possible = check_partial_downloadable(task.url.clone()).await;
        match chunk_download_possible {
            Ok(chunk_download_possible) => {
                if chunk_download_possible {
                    self.async_download_url_chunks(task.clone()).await;
                } else {
                    println!("Serial Download");
                    // self.async_serial_download(&task)
                }
            }
            Err(e) => {
                eprint!("{:?}", e);
            }
        }
        Ok(())
    }
    pub async fn async_download_url_chunks(
        &mut self,
        task: Arc<DownloadTask>,
    ) -> Result<(), AsyncErrors> {
        if (task).chunk_config.is_some() {
            chunk_downloader(task.clone(), None, self.failed_sender.clone()).await;
        }
        // Get content length
        let client = reqwest::Client::new();
        let url = (*task.url).clone();
        let response = client.head(url).send().await.expect("Cannot Fetch headers");
        let length = match response.headers().get(CONTENT_LENGTH) {
            Some(l) => Ok(l),
            None => Err(AsyncErrors::RangesUnsupported),
        };
        let length = match length.unwrap().to_str().unwrap().parse::<u64>() {
            Ok(l) => l,
            Err(_) => return Err(AsyncErrors::ParseInt),
        };

        //

        let mut handles = Vec::new();

        //Calculate chunk length
        let connection_limit = Arc::new(Semaphore::new(task.multi_connection));

        println!("starting download...");
        let r_iter = PartialRangeIter::new(0, length - 1, task.chunk_size as u32).unwrap();
        for (i, range) in r_iter.enumerate() {
            println!("{:?}", connection_limit.available_permits());
            let task = task.clone();
            let failed_sender = self.failed_sender.clone();
            let chunk_config = ChunkConfig {
                bytes_range: [range.0, range.1],
                i,
            };
            let permit = connection_limit.clone().acquire_owned().await.unwrap();
            handles.push(self.rt.spawn(async move {
                chunk_downloader(task.clone(), Some(chunk_config), failed_sender.clone()).await;
                drop(permit);
            }));
            //println!("chunk :{i}");
        }

        for handle in handles {
            tokio::join!(handle);
            println!("joining");
        }
        println!("finished downloading");
        self.task_rx.close();
        Ok(())
        // h.join();
    }
}

//Impl end
//
//

pub async fn chunk_downloader(
    task: Arc<DownloadTask>,
    chunk_config: Option<ChunkConfig>,
    failed_sender: tokio::sync::mpsc::Sender<DownloadTask>,
) {
    if let Some(chunk_config) = chunk_config {
        // original chunk download

        let f_start: u64 = (chunk_config.i * task.chunk_size) as u64;
        let client = reqwest::Client::new();
        let r = client
            .get((*task.url).clone())
            .header(RANGE, chunk_config.header_value())
            .send()
            .await;
        match r {
            Ok(mut response) => {
                let mut n = 0;
                while let Some(chunk) = response.chunk().await.unwrap() {
                    //let chunk = response.bytes().await.unwrap();
                    task.out_file.write_at(chunk.as_ref(), f_start + n);
                    n += chunk.len() as u64;
                    if n < 100000 || n > 900000 {
                        println!(
                            "chunk {} n:{n} done {:?}",
                            &chunk_config.i,
                            &chunk_config.header_value()
                        );
                    }
                }
            }
            Err(_) => {
                let task: DownloadTask = DownloadTask {
                    chunk_size: task.chunk_size,
                    url: task.url.clone(),
                    multi_connection: task.multi_connection,
                    chunk_config: Some(chunk_config),
                    out_file: task.out_file.try_clone().unwrap(),
                };
                failed_sender.send(task);
            }
        };
    }
    // Failed Chunk DownloadTask
    else {
        let chunk_config = task.chunk_config.unwrap();
        let f_start: u64 = (chunk_config.i * task.chunk_size) as u64;
        let client = reqwest::Client::new();
        let r = client
            .get((*task.url).clone())
            .header(RANGE, chunk_config.header_value())
            .send()
            .await;
        match r {
            Ok(mut response) => {
                let n = 0;
                while let Some(chunk) = response.chunk().await.unwrap() {
                    task.out_file.write_at(chunk.as_ref(), f_start + n);

                    println!("failed chunk{} n:{n} done", &chunk_config.i);
                }
            }
            Err(a) => {
                let task: DownloadTask = DownloadTask {
                    chunk_size: task.chunk_size,
                    url: task.url.clone(),
                    multi_connection: task.multi_connection,
                    chunk_config: Some(chunk_config),
                    out_file: task.out_file.try_clone().unwrap(),
                };
                failed_sender.send(task);
            }
        };
    }
}
async fn chunk_response(
    task: &Arc<DownloadTask>,
    chunk_config: &ChunkConfig,
) -> Result<Response, reqwest::Error> {
    let client = reqwest::Client::new();
    client
        .get((*task.url).clone())
        .header(RANGE, chunk_config.header_value())
        .send()
        .await
}

async fn check_partial_downloadable(url: Arc<String>) -> Result<bool, AsyncErrors> {
    let client = reqwest::Client::new();
    let range = "bytes=0-1";
    match client.get((*url).clone()).header(RANGE, range).send().await {
        Ok(r) => {
            let status = r.status();
            if !(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT) {
                eprintln!("Link is not resumable.");
                Ok(false)
            } else {
                Ok(true)
            }
        }
        Err(_) => {
            eprintln!("Failed to connect.");
            Err(AsyncErrors::ReqFetchError)
        }
    }
}

// pub fn async_file_aggregator(mut rx: std::sync::mpsc::Receiver<(usize, Bytes)>, out_file: String) {
//     println!("Called");
//     let mut rec: Vec<_> = Vec::new();
//     while let Ok(data) = rx.recv() {
//         println!("Received");
//         rec.push(data);
//     }
//     rec.sort_by_key(|item| item.0);
//     println!("Sorted");
//     let mut out_file = OpenOptions::new()
//         .create(true)
//         .append(true)
//         .write(true)
//         .open(out_file)
//         .unwrap();
//     for i in rec {
//         let _ = out_file.write(i.1.to_vec().as_slice());
//     }
//     println!("Finished Writing to file");
// }
