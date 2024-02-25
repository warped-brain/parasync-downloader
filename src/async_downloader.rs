use bytes::Bytes;
use reqwest::header::{HeaderValue, CONTENT_LENGTH, RANGE};
use reqwest::StatusCode;
use std::fs::File;
use std::io::{self, Read};
use std::sync::Arc;
use std::usize;
use std::{fs::OpenOptions, io::Write};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
#[derive(Debug)]
pub enum AsyncErrors {
    INVALID_CHUNK_SIZE,
    UNREADABLE_BYTES,
    REQ_FETCH_ERROR,
    RANGES_UNSUPPORTED,
    PARSE_INT,
    UNEXPECTED_SERVER_RESPONSE,
}
// Rust Cookbook
#[derive(Debug)]
struct PartialRangeIter {
    start: u64,
    end: u64,
    buffer_size: u32,
}

impl PartialRangeIter {
    pub fn new(start: u64, end: u64, buffer_size: u32) -> Result<Self, AsyncErrors> {
        if buffer_size == 0 {
            return Err(AsyncErrors::INVALID_CHUNK_SIZE);
        }
        Ok(PartialRangeIter {
            start,
            end,
            buffer_size,
        })
    }
}

impl Iterator for PartialRangeIter {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        if self.start > self.end {
            None
        } else {
            let prev_start = self.start;
            self.start += std::cmp::min(self.buffer_size as u64, self.end - self.start + 1);
            Some(format!("bytes={}-{}", prev_start, self.start - 1))
        }
    }
}

pub struct AsyncDownloader {
    rt: Runtime,
    sx: std::sync::mpsc::Sender<(usize, bytes::Bytes)>,
    failed: Arc<Mutex<Vec<(usize, String)>>>,
    // rx: tokio::sync::mpsc::Receiver<(usize, bytes::Bytes)>,
}
impl AsyncDownloader {
    pub fn new(
        rt: tokio::runtime::Runtime,
        sx: std::sync::mpsc::Sender<(usize, bytes::Bytes)>,
        //rx: tokio::sync::mpsc::Receiver<(usize, bytes::Bytes)>,
    ) -> Self {
        Self {
            rt,
            sx,
            failed: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub async fn async_download_url(
        self,
        url: &String,
        out_file: &String,
    ) -> Result<(), AsyncErrors> {
        match reqwest::blocking::get(url) {
            Ok(r) => {
                // println!("{:?}", r.bytes());
                let _ = std::fs::write(
                    out_file,
                    match r.bytes() {
                        Ok(bytes) => bytes,
                        Err(_) => return Err(AsyncErrors::UNREADABLE_BYTES),
                    },
                );
                Ok(())
            }
            Err(e) => Err(AsyncErrors::REQ_FETCH_ERROR),
        }
    }
    pub fn async_download_url_chunks(
        self,
        url: &String,
        // out_file: String,
        chunk_size: u32,
    ) -> Result<(), AsyncErrors> {
        // let mut out_file = OpenOptions::new().append(true).open(out_file).unwrap();
        let client = reqwest::blocking::Client::new();
        let response = client.head(url).send().expect("Cannot Fetch headers");
        let length = match response.headers().get(CONTENT_LENGTH) {
            Some(l) => Ok(l),
            None => Err(AsyncErrors::RANGES_UNSUPPORTED),
        };
        let length = match length.unwrap().to_str().unwrap().parse::<u64>() {
            Ok(l) => l,
            Err(_) => return Err(AsyncErrors::PARSE_INT),
        };
        let mut handles = Vec::new();

        // let h = std::thread::spawn(|| async_file_aggregator(self.rx, out_file));
        //println!("starting download...");
        let mut r_iter = PartialRangeIter::new(0, length - 1, chunk_size).unwrap();
        for (i, range) in r_iter.enumerate() {
            let sx = self.sx.clone();
            let url = url.clone();
            handles.push(
                self.rt
                    .spawn(chunk_downloader(range, i, url, sx, self.failed.clone())),
            );
        }
        while let Some(x) = self.failed.blocking_lock().pop() {
            let sx = self.sx.clone();
            let url = url.clone();

            handles.push(
                self.rt
                    .spawn(chunk_downloader(x.1, x.0, url, sx, self.failed.clone())),
            );
        }

        for handle in handles {
            self.rt.block_on(handle).unwrap();
        }
        // h.join();

        Ok(())
    }
}
pub async fn chunk_downloader(
    range: String,
    i: usize,
    url: String,
    sx: std::sync::mpsc::Sender<(usize, bytes::Bytes)>,
    failed: Arc<Mutex<Vec<(usize, String)>>>,
) {
    let client = reqwest::Client::new();
    let mut response = match client.get(url).header(RANGE, &range).send().await {
        Ok(r) => {
            let status = r.status();
            if !(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT) {
                eprintln!("Failed to fetch {i}th chunk.");
                failed.lock().await.push((i, range));
                return;
            };
            sx.send((i, r.bytes().await.unwrap()));

            println!("{i} range {:?}", &range);
        }
        Err(_) => {
            eprintln!("Failed to fetch {i}th chunk.");
        }
    };
}

pub fn async_file_aggregator(mut rx: std::sync::mpsc::Receiver<(usize, Bytes)>, out_file: String) {
    println!("Called");
    let mut rec: Vec<_> = Vec::new();
    while let Ok(data) = rx.recv() {
        println!("Received");
        rec.push(data);
    }
    rec.sort_by_key(|item| item.0);
    println!("Sorted");
    let mut out_file = OpenOptions::new()
        .create(true)
        .append(true)
        .write(true)
        .open(out_file)
        .unwrap();
    for i in rec {
        let _ = out_file.write(i.1.to_vec().as_slice());
    }
    println!("Finished Writing to file");
}
