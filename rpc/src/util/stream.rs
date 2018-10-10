use futures::Stream;
use std::future::set_task_waker;
use std::marker::Unpin;
use std::ops::{Generator, GeneratorState};
use std::pin::Pin;
use std::task::{LocalWaker, Poll};

/// Wrap a future in a generator.
///
/// This function returns a `GenStream` underneath, but hides it in `impl Trait` to give
/// better error messages (`impl Stream` rather than `GenStream<[closure.....]>`).
pub fn from_generator<U, T: Generator<Yield = Poll<U>, Return = ()>>(
    x: T,
) -> impl Stream<Item = U> {
    GenStream(x)
}

/// A wrapper around generators used to implement `Stream` for `async`/`await` code.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct GenStream<T>(T);

// We rely on the fact that async/await streams are immovable in order to create
// self-referential borrows in the underlying generator.
impl<U, T: Generator<Yield = Poll<U>, Return = ()>> !Unpin for GenStream<T> {}

impl<U, T: Generator<Yield = Poll<U>, Return = ()>> Stream for GenStream<T> {
    type Item = U;
    fn poll_next(self: Pin<Self>, lw: &LocalWaker) -> Poll<Option<Self::Item>> {
        set_task_waker(lw, || {
            match unsafe { Pin::get_mut_unchecked(self).0.resume() } {
                GeneratorState::Yielded(Poll::Ready(item)) => Poll::Ready(Some(item)),
                GeneratorState::Yielded(Poll::Pending) => Poll::Pending,
                GeneratorState::Complete(()) => Poll::Ready(None),
            }
        })
    }
}
