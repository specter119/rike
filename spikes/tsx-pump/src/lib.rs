//! The app's Rust code: what `App.tsx` imports from `probe/native`. Their
//! TypeScript declarations are generated from these types into
//! `.nsh/types/probe/native.d.ts`.

use nsh_base::theme;
use nsh_core::prelude::*;

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("probe/native")]
fn native(registry: &mut Registry) {
    registry
        .register::<Badge>()
        .module::<Format>()
        .class::<Counter>()
        .module::<pump::Pump>();
}

// A component: `<Badge label="…" onPress={…} />`. The function's doc comment
// is the component's in TS, the fields' its props'.
/// The props of [`Badge`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct BadgeProps {
    /// The text.
    pub label: Prop<SharedString>,
}

/// The events of a [`Badge`].
#[derive(Debug, Clone, PartialEq, Event)]
pub enum BadgeEvent {
    /// The badge was pressed.
    Press,
}

/// A pill with a label; pressing it reports `onPress`.
#[component]
pub fn Badge(props: BadgeProps, cx: Scope<BadgeEvent>) -> impl IntoElement {
    view()
        .px_4()
        .py_2()
        .rounded_full()
        .bg(move || theme().primary)
        .active(|s| s.bg(move || theme().primary.opacity(0.9)))
        .on_press(move || cx.emit(BadgeEvent::Press))
        .child(text(props.label).text_color(move || theme().primary_foreground))
}

/// Text helpers.
#[module]
pub struct Format;

// A module: `Format.times(3)`. The struct's doc comment is the module's in
// TS (`#[module]` on it and on this block).
#[module]
impl Format {
    /// "once", "twice", "3 times".
    pub fn times(count: u32) -> String {
        match count {
            1 => "once".to_owned(),
            2 => "twice".to_owned(),
            n => format!("{n} times"),
        }
    }
}

/// A count kept in Rust.
#[class]
pub struct Counter {
    count: u32,
}

// A class: `new Counter()`, `counter.increment()`, `counter.count`.
#[class]
impl Counter {
    /// A counter at 0.
    pub fn new() -> Self {
        Counter { count: 0 }
    }

    /// Adds one; the new count.
    pub fn increment(&mut self) -> u32 {
        self.count += 1;
        self.count
    }

    /// The count.
    #[getter]
    pub fn count(&self) -> u32 {
        self.count
    }
}

// ---- Resident pump probe ----
mod pump {
    use super::*;
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex, OnceLock};
    use tokio::sync::{Mutex as AsyncMutex, mpsc};

    type Rx = Arc<AsyncMutex<mpsc::UnboundedReceiver<u32>>>;

    struct Global {
        tx: mpsc::UnboundedSender<u32>,
        rx: Rx,
    }
    static GLOBAL: OnceLock<Global> = OnceLock::new();
    static STARTS: Mutex<u32> = Mutex::new(0);

    fn rt() -> &'static tokio::runtime::Runtime {
        static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_time().build().unwrap()
        })
    }

    fn global() -> &'static Global {
        GLOBAL.get_or_init(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            Global { tx, rx: Arc::new(AsyncMutex::new(rx)) }
        })
    }

    thread_local! {
        static CB: RefCell<Option<(u64, Callback<u32>)>> = const { RefCell::new(None) };
        static BUF: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
        static NEXT: RefCell<u64> = const { RefCell::new(0) };
    }

    fn deliver(ev: u32) {
        let cb = CB.with(|c| c.borrow().as_ref().map(|(_, cb)| cb.clone()));
        match cb {
            Some(cb) => cb.post(ev),
            None => BUF.with(|b| b.borrow_mut().push(ev)),
        }
    }

    /// Probe module.
    #[module]
    pub struct Pump;

    #[module]
    impl Pump {
        /// Registers the listener; events buffered while none was registered replay now.
        pub fn on_event(listener: Callback<u32>) -> Stop {
            let id = NEXT.with(|n| {
                let mut n = n.borrow_mut();
                *n += 1;
                *n
            });
            let buffered = BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
            CB.with(|c| *c.borrow_mut() = Some((id, listener.clone())));
            for ev in buffered {
                listener.post(ev);
            }
            Stop::new(move || {
                CB.with(|c| {
                    let mut c = c.borrow_mut();
                    if matches!(&*c, Some((cur, _)) if *cur == id) {
                        *c = None;
                    }
                });
            })
        }

        /// Resident task: forwards channel events to the listener until the VM goes.
        pub async fn run() {
            *STARTS.lock().unwrap() += 1;
            let rx = global().rx.clone();
            loop {
                let ev = rx.lock().await.recv().await;
                match ev {
                    Some(ev) => deliver(ev),
                    None => break,
                }
            }
        }

        /// How many times `run` started.
        pub fn starts() -> u32 {
            *STARTS.lock().unwrap()
        }

        /// Sends `count` events from a plain thread, 5 ms apart, starting at `from`.
        pub fn spawn_source(from: u32, count: u32) {
            let tx = global().tx.clone();
            std::thread::spawn(move || {
                for i in 0..count {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    let _ = tx.send(from + i);
                }
            });
        }

        /// Awaits a task on the main-owned runtime from a core task.
        pub async fn on_runtime() -> Result<u32, String> {
            let h = rt().handle().clone();
            h.spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                42u32
            })
            .await
            .map_err(|e| e.to_string())
        }

        /// Sync fn spawning onto the main-owned runtime (no tokio context here).
        pub fn spawn_on_runtime(from: u32) {
            let tx = global().tx.clone();
            let h = rt().handle().clone();
            h.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                let _ = tx.send(from);
            });
        }

        /// Sync: mutates thread-local state and returns a DTO in one call.
        pub fn bump() -> Vec<Row> {
            NEXT.with(|n| *n.borrow_mut() += 0);
            vec![Row { id: "a".into(), rev: 1 }, Row { id: "b".into(), rev: 2 }]
        }
    }

    #[derive(Debug, Clone, JsObject)]
    pub struct Row {
        pub id: String,
        pub rev: u32,
    }
}
