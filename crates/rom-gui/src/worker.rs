use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui::Context;
use rom_core::db::Database;

pub struct Worker<T> {
    db: Arc<Mutex<Database>>,
    tx: Sender<(bool, T)>,
    rx: Receiver<(bool, T)>,
    busy: Option<String>,
}

impl<T: Send + 'static> Worker<T> {
    pub fn new(db: Database) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            db: Arc::new(Mutex::new(db)),
            tx,
            rx,
            busy: None,
        }
    }

    pub fn db(&self) -> MutexGuard<'_, Database> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn busy(&self) -> Option<&str> {
        self.busy.as_deref()
    }

    pub fn run(
        &mut self,
        ctx: &Context,
        label: String,
        job: impl FnOnce(&mut Database) -> T + Send + 'static,
    ) {
        self.busy = Some(label);
        let db = Arc::clone(&self.db);
        self.spawn(ctx, true, move || {
            let mut db = db.lock().unwrap_or_else(|e| e.into_inner());
            job(&mut db)
        });
    }

    pub fn run_quiet(
        &self,
        ctx: &Context,
        job: impl FnOnce(&Mutex<Database>) -> T + Send + 'static,
    ) {
        let db = Arc::clone(&self.db);
        self.spawn(ctx, false, move || job(&db));
    }

    fn spawn(&self, ctx: &Context, busy: bool, job: impl FnOnce() -> T + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((busy, job()));
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) -> Option<T> {
        let (busy, result) = self.rx.try_recv().ok()?;
        if busy {
            self.busy = None;
        }
        Some(result)
    }
}
