use clap::{
    crate_authors, crate_description, crate_name, crate_version, value_t_or_exit, App, AppSettings,
    Arg,
};
use fuse::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use lazy_static::lazy_static;
use libc::ENOENT;
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
    static ref MEMEFSCONFIG: Mutex<MemeFSConfig> = Default::default();
    static ref REQ_CLIENT: Mutex<reqwest::Client> = Mutex::new(reqwest::Client::new());
}

#[derive(Clone, Debug, Default)]
pub struct MemeFSConfig {
    mountpoint: String,
    verbose: bool,
    subreddit: String,
    limit: u16,
    refresh_secs: u32,
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
                .find(|(_, post)| post.title == name.to_str().unwrap());
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
            let memes = MEMES
                .lock()
                .expect("Couldn't acquire lock to MEMES in read()");
            let entry = (*memes)
                .iter()
                .enumerate()
                .find(|(i, _)| (*i + 2) == ino as usize);
            if let Some((_, post)) = entry {
                let mut body_buf: Vec<u8> = vec![];
                let req_client = REQ_CLIENT
                    .lock()
                    .expect("Couldn't acquire lock to REQ_CLIENT in read()");
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

fn get_memes(memefs_config: &MemeFSConfig) -> Vec<Post> {
    let req_client = REQ_CLIENT
        .lock()
        .expect("Couldn't acquire lock to REQ_CLIENT in get_memes()");
    let resp: Value = req_client
        .get(&format!(
            "{subreddit}/.json?limit={limit}",
            subreddit = memefs_config.subreddit,
            limit = memefs_config.limit
        ))
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
                        .get("content-length")
                        .map(|cl| cl.to_str().unwrap().parse::<u64>().unwrap())
                        .unwrap_or(0),
                };
                memes.push(meme);
            }
        }
    }
    memes
}

fn parse_args() {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .about(crate_description!())
        .author(crate_authors!())
        .setting(AppSettings::ColoredHelp)
        .arg(
            Arg::with_name("MOUNTPOINT")
                .help("Choose the mountpoint")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("verbose")
                .help("Be verbose")
                .short("v")
                .long("verbose"),
        )
        .arg(
            Arg::with_name("subreddit")
                .help("Pick a subreddit or multi (requires subreddit URL)")
                .short("s")
                .default_value("https://www.reddit.com/user/Hydrauxine/m/memes")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("limit")
                .help("How many memes to fetch at once")
                .short("l")
                .default_value("20")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("refresh_secs")
                .help("How often to refresh your memes in secs")
                .short("r")
                .default_value("600")
                .takes_value(true),
        )
        .get_matches();
    let mountpoint = matches.value_of("MOUNTPOINT").unwrap().to_owned();
    let verbose = matches.is_present("verbose");
    let subreddit = matches.value_of("subreddit").unwrap().to_owned();
    let limit = value_t_or_exit!(matches.value_of("limit"), u16);
    let refresh_secs = value_t_or_exit!(matches.value_of("refresh_secs"), u32);

    {
        let mut config = MEMEFSCONFIG
            .lock()
            .expect("Couldn't acquire lock to MEMEFSCONFIG in main()");
        config.mountpoint = mountpoint;
        config.verbose = verbose;
        config.subreddit = subreddit;
        config.limit = limit;
        config.refresh_secs = refresh_secs;
    }
}

fn main() {
    parse_args();

    let options = ["-o", "ro", "-o", "fsname=memefs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    let _thread_handle = thread::spawn(move || loop {
        let config = MEMEFSCONFIG
            .lock()
            .expect("Couldn't acquire lock to MEMEFSCONFIG in main()")
            .clone();

        {
            let mut memes = MEMES
                .lock()
                .expect("Couldn't acquire lock to MEMES in main()");
            if config.verbose {
                println!("Refreshing memes");
            }
            let new_memes = get_memes(&config);
            memes.clear();
            memes.extend(new_memes);
        }

        thread::sleep(Duration::from_secs(config.refresh_secs.into()));
    });

    let config = MEMEFSCONFIG
        .lock()
        .expect("Couldn't acquire lock to MEMEFSCONFIG in main()")
        .clone();
    unsafe {
        println!("Mounting to {}", config.mountpoint);
        let _fuse_handle = fuse::spawn_mount(MemeFS, &config.mountpoint, &options).unwrap();

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
        }
        println!("Unmounting and exiting");
    }
}
