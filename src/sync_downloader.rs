use reqwest::header::{CONTENT_LENGTH, RANGE};
use reqwest::StatusCode;
use std::fs::File;
use std::io::{Read};
use std::{io::Write};

#[derive(Debug)]
pub enum SyncErrors {
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
    pub fn new(start: u64, end: u64, buffer_size: u32) -> Result<Self, SyncErrors> {
        if buffer_size == 0 {
            return Err(SyncErrors::INVALID_CHUNK_SIZE);
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
pub fn download_url(url: &String, out_file: &String) -> Result<(), SyncErrors> {
    match reqwest::blocking::get(url) {
        Ok(r) => {
            // println!("{:?}", r.bytes());
            let _ = std::fs::write(
                out_file,
                match r.bytes() {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(SyncErrors::UNREADABLE_BYTES),
                },
            );
            Ok(())
        }
        Err(_e) => Err(SyncErrors::REQ_FETCH_ERROR),
    }
}
pub fn download_url_chunks(
    url: &String,
    out_file: &String,
    chunk_size: u32,
) -> Result<(), SyncErrors> {
    let mut out_file = File::create(out_file).unwrap();
    // let mut out_file = OpenOptions::new().append(true).open(out_file).unwrap();
    let client = reqwest::blocking::Client::new();
    let response = client.head(url).send().expect("Cannot Fetch headers");
    let length = match response.headers().get(CONTENT_LENGTH) {
        Some(l) => Ok(l),
        None => Err(SyncErrors::RANGES_UNSUPPORTED),
    };
    let length = match length.unwrap().to_str().unwrap().parse::<u64>() {
        Ok(l) => l,
        Err(_) => return Err(SyncErrors::PARSE_INT),
    };

    println!("starting download...");
    let r_iter = PartialRangeIter::new(0, length - 1, chunk_size).unwrap();
    for range in r_iter {
        println!("range {:?}", &range);
        let response = match client.get(url).header(RANGE, range).send() {
            Ok(r) => r,
            Err(_) => {
                return Err(SyncErrors::REQ_FETCH_ERROR);
            }
        };

        let status = response.status();
        if !(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT) {
            return Err(SyncErrors::UNEXPECTED_SERVER_RESPONSE);
        }

        let _ = out_file.write(response.bytes().unwrap().to_vec().as_slice());
    }

    // let content = response.text()?;

    Ok(())
}
