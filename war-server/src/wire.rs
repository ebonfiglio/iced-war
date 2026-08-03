use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExit};

use war_protocol::{MAX_FRAME_SIZE, decode, encode};

pub async fn write_message<W, T>(writer: &mut w, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
{
    if payload.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }

    writer.write_u32(payload.len() as u32).await?;

    writer.write_all(&payload).await?;

    Ok(())
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


