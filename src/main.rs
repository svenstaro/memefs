extern crate ctrlc;
extern crate fuse;
extern crate libc;
extern crate time;
#[macro_use]
extern crate clap;
extern crate reqwest;
extern crate serde_json;
#[macro_use]
extern crate lazy_static;

use clap::{App, Arg};
use fuse::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::ENOENT;
use reqwest::header::ContentLength;
use serde_json::Value;
use std::cmp::min;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use time::Timespec;

lazy_static! {
    static ref MEMES: Mutex<Vec<Post>> = { Mutex::new(Vec::default()) };
}

fn dir_attr(ino: u64, size: u64) -> FileAttr {
    let current_time = time::get_time();

    FileAttr {
        ino,
        size,
        blocks: 0,
        atime: current_time,
        mtime: current_time,
        ctime: current_time,
        crtime: current_time,
        kind: FileType::Directory,
        perm: 0o755,
        nlink: 2,
        uid: 0,
        gid: 0,
        rdev: 0,
        flags: 0,
    }
}

fn file_attr(ino: u64, size: u64) -> FileAttr {
    let current_time = time::get_time();

    FileAttr {
        ino,
        size,
        blocks: 0,
        atime: current_time,
        mtime: current_time,
        ctime: current_time,
        crtime: current_time,
        kind: FileType::RegularFile,
        perm: 0o444,
        nlink: 0,
        uid: 0,
        gid: 0,
        rdev: 0,
        flags: 0,
    }
}

fn read_end(data_size: u64, offset: u64, read_size: u32) -> usize {
    (offset + min(data_size - offset, read_size as u64)) as usize
}

#[derive(Debug, Clone)]
struct Post {
    title: String,
    score: i64,
    url: String,
    size: u64,
}

struct MemeFS;

impl Filesystem for MemeFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let ttl = Timespec::new(1, 0);
        let memes = MEMES.lock().expect("Couldn't acquire lock in lookup()");
        if parent == 1 {
            let entry = (*memes)
                .iter()
                .enumerate()
                .find(|(_, post)| post.title == name.to_str().unwrap().to_owned());
            if let Some((ino, post)) = entry {
                reply.entry(&ttl, &file_attr((ino + 2) as u64, post.size), 0);
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let ttl = Timespec::new(1, 0);
        let memes = MEMES.lock().expect("Couldn't acquire lock in getattr()");
        if ino == 1 {
            reply.attr(&ttl, &dir_attr(1, memes.len() as u64))
        } else {
            let entry = (*memes)
                .iter()
                .enumerate()
                .find(|(i, _)| (*i + 2) == ino as usize);
            if let Some((_, post)) = entry {
                let size = post.size;
                reply.attr(&ttl, &file_attr(ino, size))
            } else {
                reply.error(ENOENT)
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        if ino == 1 {
            reply.error(ENOENT);
        } else {
            let memes = MEMES.lock().expect("Couldn't acquire lock in read()");
            let entry = (*memes)
                .iter()
                .enumerate()
                .find(|(i, _)| (*i + 2) == ino as usize);
            if let Some((_, post)) = entry {
                let req_client = reqwest::Client::new();
                let mut body_buf: Vec<u8> = vec![];
                let bytes_written = req_client
                    .get(&post.url)
                    .send()
                    .expect("Error while fetching posts")
                    .copy_to(&mut body_buf)
                    .expect("Can't copy response body to buffer");

                let (data_buf, data_size) = (body_buf, bytes_written as u64);
                let read_size = read_end(data_size, offset as u64, size);
                reply.data(&data_buf[offset as usize..read_size]);
            } else {
                reply.error(ENOENT)
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if ino == 1 {
            if offset == 0 {
                reply.add(1, 0, FileType::Directory, ".");
                reply.add(1, 1, FileType::Directory, "..");
                let memes = MEMES.lock().unwrap();
                for (i, meme) in (*memes).iter().enumerate() {
                    reply.add(
                        (i + 2) as u64,
                        (i + 2) as i64,
                        FileType::RegularFile,
                        &meme.title,
                    );
                }
            }
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
    }
}

fn get_memes() -> Vec<Post> {
    let req_client = reqwest::Client::new();
    let resp: Value = req_client
        .get("https://www.reddit.com/user/Hydrauxine/m/memes/.json?limit=20")
        .send()
        .expect("Error while fetching posts")
        .json()
        .expect("Can't parse posts as JSON");
    let posts = resp["data"]["children"]
        .as_array()
        .expect("Reponse has no children");
    let mut memes = vec![];
    for post in posts {
        let url_str = post["data"]["url"].as_str().expect("Post has no URL");
        let url = reqwest::Url::parse(url_str).expect("Couldn't parse Post URL");
        let url_file_ext = if let Some(url_segments) = url.path_segments() {
            if let Some(last_url_segment) = url_segments.last() {
                let file = Path::new(last_url_segment);
                if let Some(ext) = file.extension() {
                    Some(ext.to_string_lossy().to_mut().to_lowercase())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(ext) = url_file_ext {
            // Static list of known file extensions for now.
            let known_extensions = vec!["png", "jpg", "jpeg", "mp4", "webm"];
            if known_extensions.contains(&ext.as_str()) {
                // If we've gotten this far, we're ready to add the meme as a file.
                let title = post["data"]["title"].as_str().expect("Post has no Title");
                let title_with_ext = format!("{title}.{ext}", title = title, ext = ext);

                let meme = Post {
                    title: title_with_ext,
                    score: post["data"]["score"].as_i64().expect("Post has no Score"),
                    url: url_str.to_owned(),
                    size: req_client
                        .head(url)
                        .send()
                        .expect("HEAD request ")
                        .headers()
                        .get::<ContentLength>()
                        .map(|cl| **cl)
                        .unwrap_or(0),
                };
                memes.push(meme);
            }
        }
    }
    memes
}

fn main() {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .about(crate_description!())
        .author(crate_authors!())
        .arg(
            Arg::with_name("MOUNTPOINT")
                .help("Choose the mountpoint")
                .required(true)
                .index(1),
        ).get_matches();
    let mountpoint = matches.value_of("MOUNTPOINT").unwrap();

    let options = ["-o", "ro", "-o", "fsname=memefs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let _thread_handle = thread::spawn(|| loop {
        {
            let mut memes = MEMES.lock().expect("Couldn't acquire lock in main()");
            let new_memes = get_memes();
            memes.clear();
            memes.extend(new_memes);
        }

        thread::sleep(Duration::from_secs(600));
    });

    unsafe {
        println!("Mounting to {}", mountpoint);
        let _fuse_handle = fuse::spawn_mount(MemeFS, &mountpoint, &options).unwrap();

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
        }
        println!("Unmounting and exiting");
    }
}
