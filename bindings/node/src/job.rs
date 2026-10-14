//! A command left running: `Job`, the `Chunk`s its output comes back in, and `ExecStream`,
//! which follows one to its end.
//!
//! Both hold a clone of their console's slot and lock it for one call at a time, so the console
//! takes other calls between two polls. `ExecStream` is not virtx's own
//! [`ExecStream`](virtx::console::ExecStream), which borrows the console for its whole life:
//! it spawns and polls through the slot instead.

use std::sync::Arc;

use napi::{
    Env,
    bindgen_prelude::{
        Buffer, ClassInstance, JavaScriptClassExt, JsObjectValue, Object, PromiseRaw, ToNapiValue,
        Unknown,
    },
    sys,
};
use napi_derive::napi;
use tokio::{runtime::Handle, sync::Mutex};
use virtx::protocol::{JobId, PollResp};

use crate::{
    console::{Slot, held, promise, release},
    error::{self, Thrown, unsigned},
};

/// How long one poll of an `ExecStream` waits for output. It answers as soon as any arrives,
/// so this bounds only how often a silent command costs a round trip.
const STREAM_WAIT_MS: u64 = 1000;

/// How many polls, of [`STREAM_WAIT_MS`] each, a killed job is given to end before it is left
/// to the server.
const REAP_POLLS: usize = 5;

/// What a job wrote since the last poll.
#[napi(object)]
pub struct Chunk {
    pub stdout: Buffer,
    pub stderr: Buffer,
    pub truncated: bool,
    /// How the job ended: set on the last chunk only, after which the job is gone.
    pub code: Option<i32>,
}

impl From<PollResp> for Chunk {
    fn from(resp: PollResp) -> Self {
        Chunk {
            stdout: resp.stdout.into(),
            stderr: resp.stderr.into(),
            truncated: resp.truncated,
            code: resp.code,
        }
    }
}

/// A spawned command. The server holds it until the poll that carries its `code`, so poll it to
/// its end or kill it first; garbage collection does not kill it.
#[napi(js_name = "Job")]
pub struct JsJob {
    console: Slot,
    runtime: Handle,
    id: JobId,
}

impl JsJob {
    pub fn new(console: Slot, runtime: Handle, id: JobId) -> Self {
        JsJob {
            console,
            runtime,
            id,
        }
    }
}

impl Drop for JsJob {
    fn drop(&mut self) {
        release(&mut self.console, &self.runtime);
    }
}

#[napi]
impl JsJob {
    #[napi(getter)]
    pub fn id(&self) -> i64 {
        self.id as i64
    }

    /// What the job wrote since the last poll, waiting up to `waitMs` when there is nothing
    /// yet; `maxBytes` bounds each stream. A job killed at its timeout rejects with
    /// `TIMED_OUT`, carrying its last bytes.
    #[napi(ts_return_type = "Promise<Chunk>")]
    pub fn poll<'env>(
        &self,
        env: &'env Env,
        wait_ms: Option<i64>,
        max_bytes: Option<i64>,
    ) -> napi::Result<PromiseRaw<'env, Chunk>> {
        let (console, id) = (self.console.clone(), self.id);
        promise(env, async move {
            let wait_ms = unsigned(wait_ms, "waitMs")?;
            let max_bytes = unsigned(max_bytes, "maxBytes")?;
            let mut slot = console.lock().await;
            let resp = held(&mut slot)?.poll(id, wait_ms, max_bytes).await;
            resp.map(Chunk::from).map_err(error::failure)
        })
    }

    /// Send `signal` (`SIGKILL` when omitted) to the job and everything it started; the next
    /// poll reports how it ended.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn kill<'env>(
        &self,
        env: &'env Env,
        signal: Option<i32>,
    ) -> napi::Result<PromiseRaw<'env, ()>> {
        let (console, id) = (self.console.clone(), self.id);
        promise(env, async move {
            let mut slot = console.lock().await;
            held(&mut slot)?
                .kill(id, signal)
                .await
                .map_err(error::failure)
        })
    }
}

/// Where an [`ExecStream`](JsExecStream) is.
enum Streaming {
    /// Not spawned yet, so a refusal to spawn rejects the first `next()`.
    Ready(Vec<String>, Option<u64>),
    Running(JobId),
    /// Ended, refused or closed: nothing left to poll.
    Done,
}

struct Stream {
    console: Slot,
    runtime: Handle,
    state: Mutex<Streaming>,
}

impl Stream {
    /// The next poll that brought anything, or `None` once the job has ended.
    async fn next(&self) -> Result<Option<PollResp>, Thrown> {
        let mut state = self.state.lock().await;
        loop {
            let job = match &mut *state {
                Streaming::Done => return Ok(None),
                Streaming::Running(job) => *job,
                Streaming::Ready(cmd, timeout_ms) => {
                    let (cmd, timeout_ms) = (std::mem::take(cmd), *timeout_ms);
                    *state = Streaming::Done;
                    let mut slot = self.console.lock().await;
                    let job = held(&mut slot)?.spawn(cmd, timeout_ms).await;
                    let job = job.map_err(error::failure)?;
                    *state = Streaming::Running(job);
                    job
                }
            };
            let answer = {
                let mut slot = self.console.lock().await;
                match held(&mut slot) {
                    Ok(console) => console
                        .poll(job, Some(STREAM_WAIT_MS), None)
                        .await
                        .map_err(error::failure),
                    Err(closed) => Err(closed.into()),
                }
            };
            // A refusal is the job's end (a timeout) or its absence: nothing more to poll.
            let chunk = answer.inspect_err(|_| *state = Streaming::Done)?;
            if chunk.code.is_some() {
                *state = Streaming::Done;
            }
            if !chunk.is_empty() {
                return Ok(Some(chunk));
            }
        }
    }

    /// Kill the job if it is still running, and collect it.
    async fn close(&self) {
        let mut state = self.state.lock().await;
        if let Streaming::Running(job) = std::mem::replace(&mut *state, Streaming::Done) {
            reap(&self.console, job).await;
        }
    }
}

/// A stream let go of while its job runs kills the job, on the runtime since this may run in a
/// garbage-collection finalizer.
impl Drop for Stream {
    fn drop(&mut self) {
        if let Streaming::Running(job) = std::mem::replace(self.state.get_mut(), Streaming::Done) {
            let console = self.console.clone();
            self.runtime.spawn(async move { reap(&console, job).await });
        }
        release(&mut self.console, &self.runtime);
    }
}

/// `SIGKILL` `job` and poll it until the server lets it go. A job already gone, or a console
/// closed or broken, leaves nothing to do.
///
/// Bounded, since it holds the console: a process that left the job's group can keep its
/// output open past the kill.
async fn reap(console: &Slot, job: JobId) {
    let mut slot = console.lock().await;
    let Some(console) = slot.as_mut() else {
        return;
    };
    if console.kill(job, None).await.is_ok() {
        for _ in 0..REAP_POLLS {
            match console.poll(job, Some(STREAM_WAIT_MS), None).await {
                Ok(chunk) if chunk.code.is_none() => {}
                _ => break,
            }
        }
    }
}

/// One step of an async iterator: `{ value: chunk, done: false }`, or `{ done: true }` with no
/// chunk. Built by hand, as a `#[napi(object)]` would add an interface to the typings.
pub struct IteratorStep(Option<Chunk>);

impl ToNapiValue for IteratorStep {
    unsafe fn to_napi_value(env: sys::napi_env, step: Self) -> napi::Result<sys::napi_value> {
        let env = Env::from_raw(env);
        let mut object = Object::new(&env)?;
        object.set("done", step.0.is_none())?;
        if let Some(chunk) = step.0 {
            object.set("value", chunk)?;
        }
        unsafe { Object::to_napi_value(env.raw(), object) }
    }
}

/// A command's output as it is written, iterated with `for await`: each `Chunk` is what arrived
/// since the last, and the last carries the `code`. A command killed at its timeout rejects
/// with `TIMED_OUT` instead, carrying its last bytes. Left by `break` (which calls `return()`),
/// or garbage-collected, before its command ended, it kills the command.
#[napi(js_name = "ExecStream")]
pub struct JsExecStream(Arc<Stream>);

impl JsExecStream {
    /// The stream as JavaScript sees it: its own async iterator.
    pub fn new<'env>(
        env: &'env Env,
        console: Slot,
        runtime: Handle,
        cmd: Vec<String>,
        timeout_ms: Option<u64>,
    ) -> napi::Result<ClassInstance<'env, Self>> {
        let stream = JsExecStream(Arc::new(Stream {
            console,
            runtime,
            state: Mutex::new(Streaming::Ready(cmd, timeout_ms)),
        }));
        let instance = stream.into_instance(env)?;
        // `Symbol` is a function, which a checked `Object` read refuses.
        let symbol: Object = env.get_global()?.get_named_property_unchecked("Symbol")?;
        let async_iterator: Unknown = symbol.get_named_property("asyncIterator")?;
        let itself = env
            .create_function_from_closure::<(), Unknown, _>("[Symbol.asyncIterator]", |ctx| {
                ctx.this::<Unknown>()
            })?;
        instance
            .as_object(env)
            .set_property(async_iterator, itself)?;
        Ok(instance)
    }
}

#[napi]
impl JsExecStream {
    #[napi(ts_return_type = "Promise<IteratorResult<Chunk, undefined>>")]
    pub fn next<'env>(&self, env: &'env Env) -> napi::Result<PromiseRaw<'env, IteratorStep>> {
        let stream = self.0.clone();
        promise(env, async move {
            Ok(IteratorStep(stream.next().await?.map(Chunk::from)))
        })
    }

    /// Kill the command if it is still running, and settle once the server has let it go.
    #[napi(
        js_name = "return",
        ts_return_type = "Promise<IteratorResult<Chunk, undefined>>"
    )]
    pub fn close<'env>(&self, env: &'env Env) -> napi::Result<PromiseRaw<'env, IteratorStep>> {
        let stream = self.0.clone();
        promise(env, async move {
            stream.close().await;
            Ok(IteratorStep(None))
        })
    }
}
