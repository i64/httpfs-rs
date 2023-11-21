mod adapter;
mod fs;

use argh::FromArgs;
use fs::HttpFS;
use fuser::{MountOption, Session};
use rayon::max_num_threads;
use std::sync::mpsc::channel;
use std::{path::Path, thread};
use ureq::Proxy;
use url::Url;

#[derive(FromArgs)]
/// Mount HTTP resources as a file system using FUSE
struct Arguments {
    #[argh(positional)]
    /// the mount point (directory) to use.
    mountpoint: String,

    #[argh(option, long = "url")]
    /// the URL to download (mutually exclusive with 'file')
    url: Option<String>,

    #[argh(option, long = "file", short = 'f')]
    /// the file to process (mutually exclusive with 'url')
    file: Option<String>,

    #[argh(option, long = "proxy", short = 'p')]
    /// the proxy server to use
    proxy: Option<String>,
}

fn new_fs_session(opt: Arguments) -> Option<Session<HttpFS>> {
    let mountpoint = Path::new(&opt.mountpoint);

    let ureq_client = {
        let mut agent_builder =
            ureq::AgentBuilder::new().max_idle_connections_per_host(max_num_threads());

        if let Some(opt_proxy) = opt.proxy {
            let proxy =
                Proxy::new(opt_proxy).expect("the provided poxy url should be a valid proxy url");
            agent_builder = agent_builder.proxy(proxy);
        }

        agent_builder.build()
    };

    let options = vec![MountOption::RO];

    let fs = {
        if let Some(filename) = opt.file {
            let file_data = std::fs::read(filename).ok()?;
            let urls: Vec<_> = String::from_utf8_lossy(&file_data)
                .lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .map(|url| Url::parse(url).expect("the provided url should be a valid url"))
                .collect();
            HttpFS::try_new(&ureq_client, urls)
        } else if let Some(url) = opt.url {
            let parsed_url = Url::parse(&url).expect("the provided url should be a valid url");
            HttpFS::try_new(&ureq_client, vec![parsed_url])
        } else {
            None
        }
    }?;

    Session::new(fs, mountpoint, &options).ok()
}
fn main() -> std::io::Result<()> {
    let opt: Arguments = argh::from_env();

    let (tx, rx) = channel();

    thread::spawn(move || {
        let mut session = new_fs_session(opt).unwrap();
        let unmounter = session.unmount_callable();
        tx.send(unmounter).unwrap();
        println!("Mounted!");
        session.run().unwrap();
    });

    let mut unmounter = rx.recv().unwrap();
    let (tx, rx) = channel();
    ctrlc::set_handler(move || tx.send(()).unwrap()).unwrap();
    rx.recv().unwrap();

    let _ = unmounter.unmount();
    Ok(())
}
