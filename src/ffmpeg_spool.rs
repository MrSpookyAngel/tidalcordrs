use songbird::input::core::io::MediaSource;
use songbird::input::{AudioStream, AudioStreamError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Duration;

pub struct FfmpegStream {
    url: String,
    spool_read_ahead_bytes: u64,
    start_position: Duration,
}

struct SpoolState {
    bytes_written: u64,
    read_pos: u64,
    done: bool,
    stopped: bool,
    error: Option<String>,
}

struct SharedSpool {
    state: std::sync::Mutex<SpoolState>,
    available: std::sync::Condvar,
    room: std::sync::Condvar,
}

struct SpoolReader {
    file: std::fs::File,
    path: std::path::PathBuf,
    shared: std::sync::Arc<SharedSpool>,
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
    writer: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl SharedSpool {
    fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(SpoolState {
                bytes_written: 0,
                read_pos: 0,
                done: false,
                stopped: false,
                error: None,
            }),
            available: std::sync::Condvar::new(),
            room: std::sync::Condvar::new(),
        }
    }

    fn finish(&self, error: Option<String>) {
        let mut state = self.state.lock().unwrap();
        state.done = true;
        state.error = error;
        self.available.notify_all();
        self.room.notify_all();
    }
}

impl SpoolReader {
    fn new(
        mut child: std::process::Child,
        spool_read_ahead_bytes: u64,
    ) -> Result<Self, AudioStreamError> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AudioStreamError::Fail("ffmpeg stdout was not piped".into()))?;

        let path = Self::create_spool_path()?;
        let mut writer_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| AudioStreamError::Fail(Box::new(error)))?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|error| AudioStreamError::Fail(Box::new(error)))?;

        let shared = std::sync::Arc::new(SharedSpool::new());
        let child = std::sync::Arc::new(std::sync::Mutex::new(child));
        let writer_shared = std::sync::Arc::clone(&shared);
        let writer_child = std::sync::Arc::clone(&child);

        let writer = std::thread::spawn(move || {
            Self::write_spool(
                stdout,
                &mut writer_file,
                writer_shared,
                writer_child,
                spool_read_ahead_bytes,
            );
        });

        Ok(Self {
            file,
            path,
            shared,
            child,
            writer: std::sync::Mutex::new(Some(writer)),
        })
    }

    fn create_spool_path() -> Result<std::path::PathBuf, AudioStreamError> {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| AudioStreamError::Fail(Box::new(error)))?
            .as_nanos();

        Ok(std::env::temp_dir().join(format!(
            "tidalcordrs-spool-{}-{timestamp}-{id}.ogg",
            std::process::id()
        )))
    }

    fn write_spool(
        mut stdout: std::process::ChildStdout,
        writer_file: &mut std::fs::File,
        shared: std::sync::Arc<SharedSpool>,
        child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
        spool_read_ahead_bytes: u64,
    ) {
        let mut buffer = [0_u8; 64 * 1024];

        loop {
            {
                let mut state = shared.state.lock().unwrap();
                while !state.stopped
                    && state.bytes_written.saturating_sub(state.read_pos) >= spool_read_ahead_bytes
                {
                    state = shared.room.wait(state).unwrap();
                }

                if state.stopped {
                    return;
                }
            }

            match stdout.read(&mut buffer) {
                Ok(0) => {
                    let status = child.lock().unwrap().wait();
                    let error = match status {
                        Ok(status) if status.success() => None,
                        Ok(status) => Some(format!("ffmpeg exited with status {status}")),
                        Err(error) => Some(format!("failed to wait for ffmpeg: {error}")),
                    };
                    shared.finish(error);
                    return;
                }
                Ok(bytes_read) => {
                    if let Err(error) = writer_file.write_all(&buffer[..bytes_read]) {
                        shared.finish(Some(format!("failed to write spool file: {error}")));
                        return;
                    }

                    if let Err(error) = writer_file.flush() {
                        shared.finish(Some(format!("failed to flush spool file: {error}")));
                        return;
                    }

                    let mut state = shared.state.lock().unwrap();
                    state.bytes_written += bytes_read as u64;
                    shared.available.notify_all();
                }
                Err(error) => {
                    shared.finish(Some(format!("failed to read ffmpeg output: {error}")));
                    return;
                }
            }
        }
    }
}

impl Read for SpoolReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            let state = self.shared.state.lock().unwrap();

            if state.read_pos < state.bytes_written {
                let read_pos = state.read_pos;
                let available = (state.bytes_written - state.read_pos) as usize;
                let read_len = buf.len().min(available);
                drop(state);

                self.file.seek(SeekFrom::Start(read_pos))?;
                let bytes_read = self.file.read(&mut buf[..read_len])?;

                let mut state = self.shared.state.lock().unwrap();
                state.read_pos += bytes_read as u64;
                self.shared.room.notify_all();
                return Ok(bytes_read);
            }

            if state.done {
                if let Some(error) = &state.error {
                    return Err(std::io::Error::other(error.clone()));
                }

                return Ok(0);
            }

            drop(self.shared.available.wait(state).unwrap());
        }
    }
}

impl Seek for SpoolReader {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::other("source does not support seeking"))
    }
}

impl MediaSource for SpoolReader {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

impl Drop for SpoolReader {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.stopped = true;
            self.shared.available.notify_all();
            self.shared.room.notify_all();
        }

        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }

        if let Ok(mut writer) = self.writer.lock()
            && let Some(writer) = writer.take()
        {
            let _ = writer.join();
        }

        let _ = std::fs::remove_file(&self.path);
    }
}

impl FfmpegStream {
    pub fn new(url: &str, spool_read_ahead_bytes: u64) -> Self {
        Self::new_at(url, spool_read_ahead_bytes, Duration::ZERO)
    }

    pub fn new_at(url: &str, spool_read_ahead_bytes: u64, start_position: Duration) -> Self {
        Self {
            url: url.to_string(),
            spool_read_ahead_bytes,
            start_position,
        }
    }

    fn spawn(&self) -> Result<std::process::Child, AudioStreamError> {
        let mut command = std::process::Command::new("ffmpeg");
        command.args([
            "-loglevel",
            "error",
            "-nostdin",
            "-reconnect",
            "1",
            "-reconnect_streamed",
            "1",
            "-reconnect_delay_max",
            "5",
        ]);

        if !self.start_position.is_zero() {
            command.arg("-ss").arg(format!(
                "{}.{:03}",
                self.start_position.as_secs(),
                self.start_position.subsec_millis()
            ));
        }

        let child = command
            .args([
                "-i", &self.url, "-vn", "-c:a", "libopus", "-f", "opus", "pipe:1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| AudioStreamError::Fail(Box::new(error)))?;

        if child.stdout.is_none() {
            return Err(AudioStreamError::Fail("ffmpeg stdout was not piped".into()));
        }

        Ok(child)
    }
}

#[serenity::async_trait]
impl songbird::input::Compose for FfmpegStream {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let child = self.spawn()?;
        self.start_position = Duration::ZERO;
        let input = SpoolReader::new(child, self.spool_read_ahead_bytes)?;
        Ok(AudioStream {
            input: Box::new(input) as Box<dyn MediaSource>,
        })
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        self.create()
    }

    fn should_create_async(&self) -> bool {
        false
    }
}
