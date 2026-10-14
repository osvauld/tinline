//! Offline simulation preview, NOT an Iced renderer or native frame-time benchmark.
mod fixture;
mod motion;
#[path = "../../crates/gcall-bench/src/spectral.rs"]
mod spectral;
use motion::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct Event {
    at: f64,
    source: usize,
    level: u8,
    audible: bool,
    end: bool,
    features: Option<ShapeFeatures>,
}
fn read_events(path: &str, receiver: usize) -> Vec<Event> {
    let text = fs::read_to_string(path).expect("level trace");
    let mut events = Vec::new();
    for line in text.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        assert!(
            f.len() == 7 || f.len() == 12,
            "invalid level/feature trace row"
        );
        if f[1].parse::<usize>().unwrap() != receiver {
            continue;
        }
        assert!(matches!(f[3], "local" | "direct" | "group" | "end"));
        events.push(Event {
            at: f[0].parse::<f64>().unwrap() / 1e6,
            source: f[2].parse().unwrap(),
            level: f[4].parse().unwrap(),
            audible: f[5] == "1" && f[6] == "1",
            end: f[3] == "end",
            features: if f.len() == 12 {
                Some(ShapeFeatures {
                    low: f[7].parse().unwrap(),
                    high: f[9].parse().unwrap(),
                    centroid_hz: f[10].parse().unwrap(),
                    onset: f[11].parse().unwrap(),
                })
            } else {
                None
            },
        });
    }
    events.sort_by(|a, b| a.at.total_cmp(&b.at));
    assert!(!events.is_empty(), "no receiver events");
    events
}

fn write_preview(path: &PathBuf, m: &Model, dark: bool, small: bool) {
    let mut out = BufWriter::new(File::create(path).unwrap());
    let (w, h) = if small {
        (480.0, 680.0)
    } else {
        (960.0, 420.0)
    };
    let (bg, ink, accent) = if dark {
        ("#171b21", "#dfded6", "#d7bb82")
    } else {
        ("#f5f2ea", "#363d45", "#92723f")
    };
    writeln!(out, "<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'><rect width='100%' height='100%' fill='{bg}'/>").unwrap();
    writeln!(out, "<text x='28' y='32' fill='{ink}' font-family='sans-serif' font-size='17'>Audio motion / simulation only</text>").unwrap();
    writeln!(out, "<text x='28' y='55' fill='{ink}' font-family='sans-serif' font-size='11'>Alexandria Montgomery-Wellington · quiet structure, shared trace</text>").unwrap();
    for treatment in 0..2 {
        let (x, y, pw, ph) = if small {
            (28.0, 88.0 + treatment as f64 * 275.0, 424.0, 235.0)
        } else {
            (28.0 + treatment as f64 * 472.0, 95.0, 430.0, 255.0)
        };
        let title = if treatment == 0 {
            "Soft curved field"
        } else {
            "Flowing particles"
        };
        writeln!(out, "<text x='{x}' y='{}' fill='{ink}' font-family='sans-serif' font-size='13'>{title}</text>", y - 10.0).unwrap();
        writeln!(out, "<g transform='translate({x},{y})' fill='none' stroke='{accent}' stroke-linecap='round' stroke-linejoin='round'>").unwrap();
        let point = |p: Point| (p.x * pw, p.y * ph);
        if treatment == 0 {
            for i in 0..CONTOURS {
                let curve = m.contour(i);
                for (width, gain) in [(8.0, 0.12), (1.4, 1.7)] {
                    let (sx, sy) = point(curve[0].start);
                    write!(
                        out,
                        "<path stroke-width='{width}' opacity='{}' d='M{sx:.3},{sy:.3}",
                        m.opacity() * gain
                    )
                    .unwrap();
                    for c in curve {
                        let (a, b) = point(c.c1);
                        let (d, e) = point(c.c2);
                        let (f, g) = point(c.end);
                        write!(out, " C{a:.3},{b:.3} {d:.3},{e:.3} {f:.3},{g:.3}").unwrap();
                    }
                    writeln!(out, "'/>").unwrap();
                }
            }
        } else {
            for p in &m.particles {
                if p.trail_len < 2 {
                    continue;
                }
                for (width, gain) in [(5.0, 0.12), (1.8, 1.9)] {
                    write!(
                        out,
                        "<path stroke-width='{width}' opacity='{}' d='",
                        m.opacity() * gain
                    )
                    .unwrap();
                    let first = (p.head + TRAIL - p.trail_len) % TRAIL;
                    for j in 0..p.trail_len {
                        let current = p.trail[(first + j) % TRAIL];
                        let previous = p.trail[(first + j.saturating_sub(1)) % TRAIL];
                        let (a, b) = point(current);
                        if j == 0 || (current.x - previous.x).abs() > 0.4 {
                            write!(out, " M{a:.3},{b:.3}").unwrap();
                        } else {
                            // Catmull-Rom -> cubic: shared tangents on continuous trail segments.
                            let mut before = p.trail[(first + j.saturating_sub(2)) % TRAIL];
                            let mut next = p.trail[(first + (j + 1).min(p.trail_len - 1)) % TRAIL];
                            if (before.x - previous.x).abs() > 0.4 {
                                before = previous;
                            }
                            if (next.x - current.x).abs() > 0.4 {
                                next = current;
                            }
                            let c1 = Point {
                                x: previous.x + (current.x - before.x) / 6.0,
                                y: previous.y + (current.y - before.y) / 6.0,
                            };
                            let c2 = Point {
                                x: current.x - (next.x - previous.x) / 6.0,
                                y: current.y - (next.y - previous.y) / 6.0,
                            };
                            let (c, d) = point(c1);
                            let (e, f) = point(c2);
                            write!(out, " C{c:.3},{d:.3} {e:.3},{f:.3} {a:.3},{b:.3}").unwrap();
                        }
                    }
                    writeln!(out, "'/>").unwrap();
                }
            }
        }
        writeln!(out, "</g>").unwrap();
    }
    writeln!(out, "<text x='28' y='{}' fill='{ink}' font-family='sans-serif' font-size='11'>Experimental · {} · simulation, not app rendering</text></svg>", h - 20.0, if m.is_reduced() { "reduced motion" } else { "continuous motion" }).unwrap();
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let arg = |key: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
    };
    let root = PathBuf::from(arg("--out").expect("--out directory"));
    fs::create_dir_all(&root).unwrap();
    let receiver = arg("--receiver").map_or(1, |s| s.parse().unwrap());
    let events = arg("--trace").map(|p| read_events(p, receiver));
    let seconds: f64 = arg("--seconds").map_or(20.0, |s| s.parse().unwrap());
    assert!(seconds > 0.0 && seconds <= 120.0);
    let fps = 30.0;
    let mut m = Model::new();
    m.set_reduced(args.iter().any(|a| a == "--reduced"));
    let small = args.iter().any(|a| a == "--small");
    let dark = args.iter().any(|a| a == "--dark");
    m.aspect = if small { 424.0 / 235.0 } else { 430.0 / 255.0 };
    let mut extractors: [spectral::Extractor; 4] =
        std::array::from_fn(|_| spectral::Extractor::new());
    let mut audio_sample = 0;
    let mut index = 0;
    let mut costs = [0u64; 3600];
    let mut allocations = 0;
    let count = (seconds * fps) as usize;
    assert!(
        count >= 1 && count <= costs.len(),
        "duration must contain 1..3600 frames"
    );
    for frame in 0..count {
        let before = ALLOCS.load(Relaxed);
        let start = Instant::now();
        if let Some(events) = &events {
            while index < events.len() && events[index].at <= m.time() {
                let e = &events[index];
                if e.end {
                    m.end_call();
                } else if let Some(f) = e.features {
                    m.feed_spectrum(e.source, e.level, e.audible, false, e.at, f);
                } else {
                    m.feed(e.source, e.level, e.audible, false, e.at);
                }
                index += 1;
            }
        } else {
            fixture::synthetic(&mut m, &mut extractors, &mut audio_sample);
        }
        m.step(1.0 / fps);
        costs[frame] = start.elapsed().as_nanos() as u64;
        allocations += ALLOCS.load(Relaxed) - before;
        write_preview(&root.join(format!("frame-{frame:04}.svg")), &m, dark, small);
    }
    costs[..count].sort_unstable();
    let p95 = costs[(count - 1) * 95 / 100];
    let max = costs[count - 1];
    let mut perf = File::create(root.join("performance.txt")).unwrap();
    writeln!(perf, "Simulation update CPU: p95={p95}ns max={max}ns allocations={allocations}; fixed Model={}B; one Extractor={}B; {count} frames at {fps}fps.\nSynthetic mode includes feature extraction; recorded mode only replays descriptors. Excludes SVG I/O, rasterization, renderer/GPU, layout and audio callbacks. Not native frame time or Android battery evidence.", std::mem::size_of::<Model>(), std::mem::size_of::<spectral::Extractor>()).unwrap();
    println!("model p95 {p95}ns; max {max}ns; allocations {allocations}; frames {count}");
}
