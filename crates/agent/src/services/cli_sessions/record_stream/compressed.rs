use super::*;
use crate::services::cli_sessions::workbench::BudgetFile;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

type Decoder = BufReader<zstd::stream::read::Decoder<'static, BufReader<BudgetFile>>>;
struct Stream {
    path: PathBuf,
    fingerprint: String,
    position: u64,
    reader: Decoder,
}
fn pool() -> &'static Mutex<VecDeque<Stream>> {
    static POOL: OnceLock<Mutex<VecDeque<Stream>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(VecDeque::new()))
}
struct Lease(Option<Stream>, bool);
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.1 {
            return;
        }
        if let Some(stream) = self.0.take() {
            let mut pool = pool().lock().unwrap_or_else(|error| error.into_inner());
            pool.retain(|item| item.path != stream.path);
            pool.push_back(stream);
            while pool.len() > 4 {
                pool.pop_front();
            }
        }
    }
}

pub(super) fn read_batch(
    source: &SessionIndexSource,
    previous: Option<&Checkpoint>,
    current: &dyn Fn() -> bool,
    batch_bytes: u64,
) -> Result<Batch, String> {
    if !current() {
        return Err("会话读取已取消".into());
    }
    let file = super::super::workbench::open_session_file(&source.path)
        .map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let identity = identity(&metadata);
    let fingerprint = fingerprint(&metadata, source.parser_version);
    let reusable = previous.is_some_and(|checkpoint| {
        checkpoint.identity == identity && checkpoint.fingerprint == fingerprint
    });
    if reusable && previous.is_some_and(|checkpoint| checkpoint.complete) {
        return Ok(Batch {
            checkpoint: previous.unwrap().clone(),
            reset: false,
            mutations: Vec::new(),
        });
    }
    let mut checkpoint = if reusable {
        previous.cloned().unwrap_or_default()
    } else {
        Checkpoint::default()
    };
    checkpoint.identity = identity;
    checkpoint.fingerprint = fingerprint.clone();
    checkpoint.source_bytes = metadata.len();
    let cached = {
        let mut pool = pool().lock().unwrap_or_else(|error| error.into_inner());
        pool.iter()
            .position(|stream| {
                stream.path == source.path
                    && stream.fingerprint == fingerprint
                    && stream.position <= checkpoint.offset
            })
            .and_then(|index| pool.remove(index))
    };
    let stream = match cached {
        Some(mut stream) => {
            stream
                .reader
                .get_mut()
                .get_mut()
                .get_mut()
                .use_current_budget();
            stream
        }
        None => Stream {
            path: source.path.clone(),
            fingerprint,
            position: 0,
            reader: BufReader::new(
                zstd::stream::read::Decoder::new(file).map_err(|error| error.to_string())?,
            ),
        },
    };
    let mut lease = Lease(Some(stream), false);
    let stream = lease.0.as_mut().ok_or("压缩会话读取未初始化")?;
    // Keep the decoder between batches. After a process restart only the first
    // batch replays the compressed prefix, with cancellation at every buffer.
    while stream.position < checkpoint.offset {
        if !current() {
            return Err("会话读取已取消".into());
        }
        let bytes = stream
            .reader
            .fill_buf()
            .map_err(|error| error.to_string())?;
        if bytes.is_empty() {
            return Err("压缩会话在读取期间发生变化".into());
        }
        let length = bytes
            .len()
            .min((checkpoint.offset - stream.position) as usize);
        stream.reader.consume(length);
        stream.position += length as u64;
    }
    let started = Instant::now();
    let start = checkpoint.offset;
    let mut mutations = Vec::new();
    loop {
        if !current() {
            return Err("会话读取已取消".into());
        }
        if checkpoint.offset > start
            && (checkpoint.offset - start >= batch_bytes || started.elapsed() >= BATCH_TIME)
        {
            break;
        }
        let next = match next_line(&mut stream.reader, &mut checkpoint) {
            Ok(next) => next,
            Err(error) => {
                lease.0 = None;
                return Err(error);
            }
        };
        stream.position = checkpoint.offset;
        match next {
            Record::Value(bytes) => {
                for mutation in (source.decode)(
                    &source.path,
                    checkpoint.sequence,
                    &bytes,
                    &mut checkpoint.decoder,
                ) {
                    checkpoint.mutation_order += 1;
                    mutations.push((checkpoint.mutation_order, mutation));
                }
                checkpoint.sequence += 1;
            }
            Record::Continue => {}
            Record::End => {
                checkpoint.complete = true;
                break;
            }
        }
    }
    let after = super::super::workbench::open_session_file(&source.path)
        .and_then(|file| file.metadata())
        .map_err(|error| error.to_string())?;
    if super::identity(&after) != checkpoint.identity
        || super::fingerprint(&after, source.parser_version) != checkpoint.fingerprint
    {
        lease.0 = None;
        return Err("压缩会话在读取期间发生变化，请重试".into());
    }
    if checkpoint.complete {
        lease.0 = None;
    }
    lease.1 = true;
    Ok(Batch {
        checkpoint,
        reset: !reusable,
        mutations,
    })
}
