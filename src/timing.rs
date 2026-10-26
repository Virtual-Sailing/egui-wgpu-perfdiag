//! Optional, bounded frame instrumentation. No GPU waits or synchronous readbacks.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender, sync_channel},
};
use std::time::Instant;

/// Non-overlapping CPU portions of the native egui painter, in seconds.
#[derive(Clone, Copy, Debug)]
pub struct PaintTimings {
    pub rendered: bool,
    pub started: Instant,
    pub finished: Instant,
    pub upload: f64,
    pub acquire: f64,
    pub encode: f64,
    pub submit: f64,
    pub present: f64,
}

pub type PaintObserver = dyn Fn(egui::ViewportId, PaintTimings) + Send + Sync;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuStage {
    Scene,
    Ui,
}

#[derive(Clone, Copy, Debug)]
pub struct GpuSample {
    pub frame: u64,
    pub stage: GpuStage,
    /// None means the query failed or every readback slot was still busy.
    pub seconds: Option<f64>,
}

/// Shared frame identity and bounded asynchronous results for both render passes.
pub struct GpuProfiler {
    pub enabled: AtomicBool,
    pub frame: AtomicU64,
    sender: SyncSender<GpuSample>,
}

impl GpuProfiler {
    pub fn new() -> (Arc<Self>, Receiver<GpuSample>) {
        let (sender, receiver) = sync_channel(128);
        (
            Arc::new(Self {
                enabled: AtomicBool::new(false),
                frame: AtomicU64::new(0),
                sender,
            }),
            receiver,
        )
    }
}

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    busy: Arc<AtomicBool>,
    frame: u64,
}

/// Native Metal completion timestamps, or four reusable query/readback slots on
/// other backends. Busy slots are skipped, never waited on.
pub struct GpuTimer {
    slots: Vec<Slot>,
    period: f64,
    profiler: Arc<GpuProfiler>,
    stage: GpuStage,
}

impl GpuTimer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        profiler: Arc<GpuProfiler>,
        stage: GpuStage,
    ) -> Self {
        let slots = if !cfg!(target_os = "macos")
            && device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            (0..4)
                .map(|_| Slot {
                    queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("frame timing"),
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    }),
                    resolve: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("timestamp resolve"),
                        size: 16,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    readback: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("timestamp readback"),
                        size: 16,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    }),
                    busy: Arc::new(AtomicBool::new(false)),
                    frame: 0,
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            slots,
            period: queue.get_timestamp_period() as f64 * 1e-9,
            profiler,
            stage,
        }
    }

    pub fn begin(&mut self) -> Option<usize> {
        // Metal command-buffer GPUStartTime/GPUEndTime avoid unreliable reused
        // end-of-fragment query samples on some Apple GPU/OS combinations.
        if cfg!(target_os = "macos") {
            return None;
        }
        if !self.profiler.enabled.load(Ordering::Relaxed) {
            return None;
        }
        let frame = self.profiler.frame.load(Ordering::Relaxed);
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !slot.busy.swap(true, Ordering::Acquire) {
                slot.frame = frame;
                return Some(index);
            }
        }
        let _ = self.profiler.sender.try_send(GpuSample {
            frame,
            stage: self.stage,
            seconds: None,
        });
        None
    }

    pub fn writes(&self, slot: usize) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.slots[slot].queries,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    pub fn resolve(&self, slot: usize, encoder: &mut wgpu::CommandEncoder) {
        let slot = &self.slots[slot];
        encoder.resolve_query_set(&slot.queries, 0..2, &slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&slot.resolve, 0, &slot.readback, 0, 16);
    }

    pub fn prepare_encoder(&self, _encoder: &mut wgpu::CommandEncoder) {
        #[cfg(target_os = "macos")]
        if self.profiler.enabled.load(Ordering::Relaxed) {
            let profiler = self.profiler.clone();
            let frame = profiler.frame.load(Ordering::Relaxed);
            let stage = self.stage;
            // SAFETY: only attach a completion observer to the live encoder.
            // Do not end, submit, retain, destroy, or otherwise alter its commands.
            let attached = unsafe {
                _encoder.as_hal_mut::<wgpu::hal::api::Metal, _, _>(|encoder| {
                    encoder.is_some_and(|encoder| {
                        encoder.on_next_render_pass_completed(move |seconds| {
                            let _ = profiler.sender.try_send(GpuSample {
                                frame,
                                stage,
                                seconds,
                            });
                        });
                        true
                    })
                })
            };
            if !attached {
                let _ = self.profiler.sender.try_send(GpuSample {
                    frame,
                    stage,
                    seconds: None,
                });
            }
        }
    }

    pub fn submitted(&self, index: usize) {
        let slot = &self.slots[index];
        let readback = slot.readback.clone();
        let busy = slot.busy.clone();
        let profiler = self.profiler.clone();
        let frame = slot.frame;
        let stage = self.stage;
        let period = self.period;
        slot.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let seconds = if result.is_ok() {
                    let data = readback.slice(..).get_mapped_range();
                    let start = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    let end = u64::from_le_bytes(data[8..16].try_into().unwrap());
                    let seconds =
                        (end >= start).then_some((end.wrapping_sub(start)) as f64 * period);
                    drop(data);
                    readback.unmap();
                    seconds
                } else {
                    None
                };
                let _ = profiler.sender.try_send(GpuSample {
                    frame,
                    stage,
                    seconds,
                });
                busy.store(false, Ordering::Release);
            });
    }
}
