// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! Transports backed by in-memory channels.

use crate::PollIo;
use futures::{channel::mpsc, prelude::*};
use pin_project::pin_project;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

/// Returns two unbounded channel peers. Each [`Stream`] yields items sent through the other's
/// [`Sink`].
pub fn unbounded<SinkItem, Item>() -> (
    UnboundedChannel<SinkItem, Item>,
    UnboundedChannel<Item, SinkItem>,
) {
    let (tx1, rx2) = mpsc::unbounded();
    let (tx2, rx1) = mpsc::unbounded();
    (
        UnboundedChannel { tx: tx1, rx: rx1 },
        UnboundedChannel { tx: tx2, rx: rx2 },
    )
}

/// A bi-directional channel backed by an [`UnboundedSender`](mpsc::UnboundedSender)
/// and [`UnboundedReceiver`](mpsc::UnboundedReceiver).
#[pin_project]
#[derive(Debug)]
pub struct UnboundedChannel<Item, SinkItem> {
    #[pin]
    rx: mpsc::UnboundedReceiver<Item>,
    #[pin]
    tx: mpsc::UnboundedSender<SinkItem>,
}

impl<Item, SinkItem> UnboundedChannel<Item, SinkItem> {
    fn rx_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut mpsc::UnboundedReceiver<Item>> {
        self.as_mut().project().rx
    }

    fn tx_pin_mut<'a>(
        self: &'a mut Pin<&mut Self>,
    ) -> Pin<&'a mut mpsc::UnboundedSender<SinkItem>> {
        self.as_mut().project().tx
    }
}

impl<Item, SinkItem> Stream for UnboundedChannel<Item, SinkItem> {
    type Item = Result<Item, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> PollIo<Item> {
        self.rx_pin_mut().poll_next(cx).map(|option| option.map(Ok))
    }
}

impl<Item, SinkItem> Sink<SinkItem> for UnboundedChannel<Item, SinkItem> {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.tx_pin_mut()
            .poll_ready(cx)
            .map_err(|_| io::Error::from(io::ErrorKind::NotConnected))
    }

    fn start_send(mut self: Pin<&mut Self>, item: SinkItem) -> io::Result<()> {
        self.tx_pin_mut()
            .start_send(item)
            .map_err(|_| io::Error::from(io::ErrorKind::NotConnected))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.tx_pin_mut()
            .poll_flush(cx)
            .map_err(|_| io::Error::from(io::ErrorKind::NotConnected))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.tx_pin_mut()
            .poll_close(cx)
            .map_err(|_| io::Error::from(io::ErrorKind::NotConnected))
    }
}
