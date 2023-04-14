use std::io::Read;

use bytes::Bytes;
use fs_err::File;
use tokio::{sync::mpsc, task::block_in_place};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tracing::warn;

const CONTENT_CHUNK_LEN: usize = 1024;

pub fn stream_file(mut file: File) -> impl Stream<Item = Bytes> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(async move {
        let mut buf = vec![0u8; CONTENT_CHUNK_LEN];
        loop {
            match block_in_place(|| file.read(&mut buf)) {
                Ok(len) => {
                    if len == 0 {
                        break; // end of file
                    } else {
                        if tx.send(Bytes::copy_from_slice(&buf[0..len])).await.is_err() {
                            break; // receiver closed
                        }
                    }
                }
                Err(err) => {
                    warn!(%err, "failed to read content file");
                    break;
                }
            }
        }
    });
    ReceiverStream::new(rx)
}
