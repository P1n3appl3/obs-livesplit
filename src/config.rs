use std::{
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    path,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use livesplit_core::{
    layout::{self, LayoutSettings},
    run::parser::composite,
    Layout, Run, Segment,
};
use log::warn;
use notify::{self, DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};

pub fn default_run() -> Run {
    let mut run = Run::new();
    run.push_segment(Segment::new("Time"));
    run
}

pub fn parse_run(path: &Path) -> Option<Run> {
    if path.to_str()?.is_empty() {
        return None;
    }
    match composite::parse(&fs::read(&path).ok()?, Some(path.to_path_buf()), true) {
        Ok(r) => {
            if r.run.is_empty() {
                None
            } else {
                Some(r.run)
            }
        }
        Err(e) => {
            warn!("{e}");
            None
        }
    }
}

pub fn parse_layout(path: &Path) -> Option<Layout> {
    if path.to_str()?.is_empty() {
        return None;
    }
    let mut reader = BufReader::new(File::open(path).ok()?);
    match LayoutSettings::from_json(&mut reader) {
        Ok(settings) => return Some(Layout::from_settings(settings)),
        Err(e) => warn!("{e}"),
    }

    // fallback to parsing old livesplit layouts
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut s = String::new();
    reader.read_to_string(&mut s).ok()?;
    match layout::parser::parse(&s) {
        Ok(l) => Some(l),
        Err(e) => {
            warn!("{e}");
            None
        }
    }
}

pub struct ConfigWatcher {
    pub watcher: RecommendedWatcher,
    pub rx: Receiver<DebouncedEvent>,
    pub path: Option<PathBuf>,
}

impl ConfigWatcher {
    pub fn new(delay: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            watcher: RecommendedWatcher::new(tx, delay).unwrap(),
            rx,
            path: None,
        }
    }

    pub fn change_file<P: AsRef<path::Path>>(&mut self, path: P) -> notify::Result<()> {
        let path = path.as_ref();
        if let Some(current) = &self.path {
            if current.as_path() == path {
                return Ok(());
            }
            self.watcher.unwatch(current.parent().unwrap())?;
        }
        self.path = Some(path.into());
        self.watcher
            .watch(path.parent().unwrap(), RecursiveMode::Recursive)
    }

    pub fn handle_updates(&mut self, mut callback: impl FnMut(PathBuf)) {
        use DebouncedEvent::*;
        while let Ok(event) = self.rx.try_recv() {
            if let Create(p) | Write(p) = event && self.path.as_deref() == Some(&p){
                callback(p)
            }
        }
    }
}
