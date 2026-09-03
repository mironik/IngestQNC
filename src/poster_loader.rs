use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
};

use eframe::egui::ColorImage;

#[derive(Debug)]
pub struct PosterLoadResult {
    pub clip_id: String,
    pub path: String,
    pub image: Result<ColorImage, String>,
}

#[derive(Debug)]
struct PosterLoadRequest {
    clip_id: String,
    path: String,
}

pub struct PosterAssetLoader {
    tx: Sender<PosterLoadRequest>,
    rx: Receiver<PosterLoadResult>,
    pending: HashSet<String>,
}

impl PosterAssetLoader {
    pub fn new() -> Self {
        let (tx, work_rx) = mpsc::channel::<PosterLoadRequest>();
        let (result_tx, rx) = mpsc::channel::<PosterLoadResult>();
        let work_rx = Arc::new(Mutex::new(work_rx));

        for ix in 0..2 {
            let work_rx = Arc::clone(&work_rx);
            let result_tx = result_tx.clone();
            thread::Builder::new()
                .name(format!("ingestqnc-poster-loader-{ix}"))
                .spawn(move || loop {
                    let request = match work_rx.lock().expect("poster loader queue").recv() {
                        Ok(request) => request,
                        Err(_) => break,
                    };
                    let image = load_color_image(&PathBuf::from(&request.path));
                    let _ = result_tx.send(PosterLoadResult {
                        clip_id: request.clip_id,
                        path: request.path,
                        image,
                    });
                })
                .expect("spawn poster loader");
        }

        Self {
            tx,
            rx,
            pending: HashSet::new(),
        }
    }

    pub fn request(&mut self, clip_id: &str, path: &str) -> bool {
        let clip_id = clip_id.trim();
        let path = path.trim();
        if clip_id.is_empty() || path.is_empty() || self.pending.contains(clip_id) {
            return false;
        }

        let request = PosterLoadRequest {
            clip_id: clip_id.to_owned(),
            path: path.to_owned(),
        };
        if self.tx.send(request).is_ok() {
            self.pending.insert(clip_id.to_owned());
            true
        } else {
            false
        }
    }

    pub fn poll(&mut self) -> Vec<PosterLoadResult> {
        let mut out = Vec::new();
        while let Ok(result) = self.rx.try_recv() {
            self.pending.remove(&result.clip_id);
            out.push(result);
        }
        out
    }

    pub fn is_pending(&self, clip_id: &str) -> bool {
        self.pending.contains(clip_id)
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

fn load_color_image(path: &PathBuf) -> Result<ColorImage, String> {
    let bytes = fs::read(path).map_err(|error| format!("poster read failed: {error}"))?;
    let image = image::load_from_memory(&bytes)
        .map_err(|error| format!("poster decode failed: {error}"))?
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}
