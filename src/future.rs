// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

impl<T: Sized> Instrument for T {}

pub trait Instrument: Sized {
    #[inline]
    fn trace_task<E: Into<u32>>(self, event: E) -> TraceSpawned<Self> {
        let event = event.into();
        self.trace_task_fine(event, event)
    }

    #[inline]
    fn trace_task_fine<E1: Into<u32>, E2: Into<u32>>(
        self,
        pending_event: E1,
        settle_event: E2,
    ) -> TraceSpawned<Self> {
        let event = settle_event.into();
        TraceSpawned {
            inner: self,
            event,
            trace_handle: Some(crate::trace::trace_binder_fine(event, pending_event)),
        }
    }

    #[inline]
    fn trace_async<T: Into<u32>>(self, event: T) -> TraceWrapped<Self> {
        TraceWrapped {
            inner: self,
            event: event.into(),
        }
    }

    #[inline]
    fn future_trace_enable<E: Into<u32>>(self, event: E) -> TraceRootFuture<Self> {
        let event = event.into();
        self.future_trace_enable_fine(event, event, event)
    }

    #[inline]
    fn future_trace_enable_fine<E0: Into<u32>, E1: Into<u32>, E2: Into<u32>>(
        self,
        event: E0,
        pending_event: E1,
        settle_event: E2,
    ) -> TraceRootFuture<Self> {
        let now_cycles = minstant::now();
        let now = crate::time::real_time_ns();
        let collector = crate::collector::Collector::new(now);

        TraceRootFuture {
            inner: self,
            event: settle_event.into(),
            trace_handle: Some(crate::trace_async::TraceHandle::new_root(
                collector.inner.clone(),
                now_cycles,
                event.into(),
                pending_event.into(),
            )),
            collector: Some(collector),
        }
    }

    #[inline]
    fn future_trace_may_enable<E: Into<u32>>(
        self,
        enable: bool,
        event: E,
    ) -> MayTraceRootFuture<Self> {
        let event = event.into();
        self.future_trace_may_enable_fine(enable, event, event)
    }

    #[inline]
    fn future_trace_may_enable_fine<E1: Into<u32>, E2: Into<u32>>(
        self,
        enable: bool,
        pending_event: E1,
        settle_event: E2,
    ) -> MayTraceRootFuture<Self> {
        if enable {
            let now_cycles = minstant::now();

            let now = crate::time::real_time_ns();
            let collector = crate::collector::Collector::new(now);
            let event = settle_event.into();
            MayTraceRootFuture {
                inner: self,
                event,
                trace_handle: Some(crate::trace_async::TraceHandle::new_root(
                    collector.inner.clone(),
                    now_cycles,
                    event,
                    pending_event.into(),
                )),
                collector: Some(collector),
            }
        } else {
            MayTraceRootFuture {
                inner: self,
                event: settle_event.into(),
                collector: None,
                trace_handle: None,
            }
        }
    }
}

#[pin_project::pin_project]
pub struct TraceSpawned<T> {
    #[pin]
    inner: T,
    event: u32,
    trace_handle: Option<crate::trace_async::TraceHandle>,
}

impl<T: std::future::Future> std::future::Future for TraceSpawned<T> {
    type Output = T::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        let guard = this
            .trace_handle
            .as_mut()
            .unwrap()
            .trace_enable(*this.event);
        match this.inner.poll(cx) {
            r @ std::task::Poll::Ready(_) => {
                drop(guard);
                drop(this.trace_handle.take());
                r
            }
            other => {
                drop(guard);
                other
            }
        }
    }
}

impl<T: futures_01::Future> futures_01::Future for TraceSpawned<T> {
    type Item = T::Item;
    type Error = T::Error;

    fn poll(&mut self) -> futures_01::Poll<Self::Item, Self::Error> {
        let guard = self.trace_handle.as_mut().unwrap().trace_enable(self.event);
        match self.inner.poll() {
            r @ Ok(futures_01::Async::NotReady) => {
                drop(guard);
                r
            }
            other => {
                drop(guard);
                drop(self.trace_handle.take());
                other
            }
        }
    }
}

#[pin_project::pin_project]
pub struct TraceWrapped<T> {
    #[pin]
    inner: T,
    event: u32,
}

impl<T: std::future::Future> std::future::Future for TraceWrapped<T> {
    type Output = T::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        let _guard = crate::trace::new_span(*this.event);
        this.inner.poll(cx)
    }
}

impl<T: futures_01::Future> futures_01::Future for TraceWrapped<T> {
    type Item = T::Item;
    type Error = T::Error;

    fn poll(&mut self) -> futures_01::Poll<Self::Item, Self::Error> {
        let _guard = crate::trace::new_span(self.event);
        self.inner.poll()
    }
}

#[pin_project::pin_project]
pub struct MayTraceRootFuture<T> {
    #[pin]
    inner: T,
    event: u32,

    trace_handle: Option<crate::trace_async::TraceHandle>,

    // finally return to user
    collector: Option<crate::collector::Collector>,
}

impl<T: std::future::Future> std::future::Future for MayTraceRootFuture<T> {
    type Output = (Option<crate::Collector>, T::Output);

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        let event = *this.event;
        let guard = this
            .trace_handle
            .as_mut()
            .and_then(|a| a.trace_enable(event));

        match this.inner.poll(cx) {
            std::task::Poll::Ready(r) => {
                drop(guard);
                drop(this.trace_handle.take());
                std::task::Poll::Ready((this.collector.take(), r))
            }
            _ => {
                drop(guard);
                std::task::Poll::Pending
            }
        }
    }
}

impl<T: futures_01::Future> futures_01::Future for MayTraceRootFuture<T> {
    type Item = (Option<crate::Collector>, T::Item);
    type Error = (Option<crate::Collector>, T::Error);

    fn poll(&mut self) -> futures_01::Poll<Self::Item, Self::Error> {
        let event = self.event;
        let guard = self
            .trace_handle
            .as_mut()
            .and_then(|a| a.trace_enable(event));

        match self.inner.poll() {
            Ok(futures_01::Async::NotReady) => {
                drop(guard);
                Ok(futures_01::Async::NotReady)
            }
            Ok(futures_01::Async::Ready(r)) => {
                drop(guard);
                drop(self.trace_handle.take());
                Ok(futures_01::Async::Ready((self.collector.take(), r)))
            }
            Err(err) => {
                drop(guard);
                drop(self.trace_handle.take());
                Err((self.collector.take(), err))
            }
        }
    }
}

#[pin_project::pin_project]
pub struct TraceRootFuture<T> {
    #[pin]
    inner: T,
    event: u32,

    trace_handle: Option<crate::trace_async::TraceHandle>,
    // finally return to user
    collector: Option<crate::collector::Collector>,
}

impl<T: std::future::Future> std::future::Future for TraceRootFuture<T> {
    type Output = (crate::Collector, T::Output);

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.project();
        let guard = this
            .trace_handle
            .as_mut()
            .unwrap()
            .trace_enable(*this.event);

        match this.inner.poll(cx) {
            std::task::Poll::Ready(r) => {
                drop(guard);
                drop(this.trace_handle.take());
                std::task::Poll::Ready((this.collector.take().unwrap(), r))
            }
            _ => {
                drop(guard);
                std::task::Poll::Pending
            }
        }
    }
}

impl<T: futures_01::Future> futures_01::Future for TraceRootFuture<T> {
    type Item = (crate::Collector, T::Item);
    type Error = (crate::Collector, T::Error);

    fn poll(&mut self) -> futures_01::Poll<Self::Item, Self::Error> {
        let guard = self.trace_handle.as_mut().unwrap().trace_enable(self.event);

        match self.inner.poll() {
            Ok(futures_01::Async::NotReady) => {
                drop(guard);
                Ok(futures_01::Async::NotReady)
            }
            Ok(futures_01::Async::Ready(r)) => {
                drop(guard);
                drop(self.trace_handle.take());
                Ok(futures_01::Async::Ready((
                    self.collector.take().unwrap(),
                    r,
                )))
            }
            Err(err) => {
                drop(guard);
                drop(self.trace_handle.take());
                Err((self.collector.take().unwrap(), err))
            }
        }
    }
}
