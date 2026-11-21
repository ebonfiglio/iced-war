use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{MAX_FRAME_SIZE, decode, encode};

pub async fn write_message<W, T>(writer: &mut W, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
{
    let payload =
        encode(message).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    if payload.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }

    let length = (payload.len() as u32).to_be_bytes();
    let mut frame = Vec::with_capacity(length.len() + payload.len());
    frame.extend_from_slice(&length);
    frame.extend_from_slice(&payload);
    writer.write_all(&frame).await
}

pub async fn read_message<R, T>(reader: &mut R) -> io::Result<T>
where
    R: AsyncRead + Unpin,
    T: serde::de::DeserializeOwned,
{
    let length = reader.read_u32().await? as usize;

    if length > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incoming message too large",
        ));
    }

    let mut payload = vec![0; length];

    reader.read_exact(&mut payload).await?;

    decode(&payload).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
