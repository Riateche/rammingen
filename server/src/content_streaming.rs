use std::{convert::Infallible, io::Write};

use futures_util::StreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, StreamBody};
use hyper::{
    body::{self, Bytes, Frame},
    header::CONTENT_LENGTH,
    Request, Response, StatusCode,
};
use rammingen_protocol::{util::stream_file, EncryptedContentHash};
use tokio::task::block_in_place;
use tracing::warn;

use crate::handler;

pub async fn upload(
    ctx: handler::Context,
    mut request: Request<body::Incoming>,
    hash: &EncryptedContentHash,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode> {
    let content_length: u64 = request
        .headers()
        .get(CONTENT_LENGTH)
        .ok_or_else(|| {
            warn!("missing content length in request");
            StatusCode::BAD_REQUEST
        })?
        .to_str()
        .map_err(|err| {
            warn!(?err, "invalid content length in request");
            StatusCode::BAD_REQUEST
        })?
        .parse()
        .map_err(|err| {
            warn!(?err, "invalid content length in request");
            StatusCode::BAD_REQUEST
        })?;

    let mut file = block_in_place(|| ctx.storage.create_file()).map_err(|err| {
        warn!(?err, "failed to create file");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut received_length = 0;
    while let Some(frame) = request.body_mut().frame().await {
        let frame = frame.map_err(|err| {
            warn!(?err, "failed to read request frame");
            StatusCode::BAD_REQUEST
        })?;
        let data = frame.data_ref().ok_or_else(|| {
            warn!("unexpected trailer frame in request");
            StatusCode::BAD_REQUEST
        })?;
        received_length += data.len() as u64;
        block_in_place(|| file.write_all(data)).map_err(|err| {
            warn!(?err, "failed to write to content file");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if content_length != received_length {
        warn!(content_length, received_length, "content length mismatch");
        return Err(StatusCode::BAD_REQUEST);
    }

    block_in_place(|| ctx.storage.commit_file(file, hash)).map_err(|err| {
        warn!(?err, "failed to commit content file");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Response::new(BodyExt::boxed(Empty::new())))
}

pub async fn download(
    ctx: handler::Context,
    hash: &EncryptedContentHash,
) -> Result<Response<BoxBody<Bytes, Infallible>>, StatusCode> {
    let file = block_in_place(|| ctx.storage.open_file(hash)).map_err(|err| {
        warn!(?err, "couldn't open content file");
        StatusCode::NOT_FOUND
    })?;
    let len = file
        .metadata()
        .map_err(|err| {
            warn!(?err, "couldn't get metadata for content file");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .len();
    Ok(Response::builder()
        .header(CONTENT_LENGTH, len)
        .body(BodyExt::boxed(StreamBody::new(
            stream_file(file).map(|bytes| Ok(Frame::data(bytes))),
        )))
        .expect("response builder failed"))
}
