use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:9001").await?;

    println!("WAR Server listening on 127.0.0.1:9001");

    loop {
        let (mut socket, address) = listener.accept().await?;

        println!("Player connected from {address}");

        tokio::spawn(async move {
            let mut buffer = [0_u8; 1024];

            loop {
                let bytes_read = match socket.read(&mut buffer).await {
                    Ok(0) => {
                        println!("{address} disconnected");
                        return;
                    }

                    Ok(n) => n,

                    Err(error) => {
                        eprintln!("Read error: {error}");
                        return;
                    }
                };

                println!(
                    "Received: {}",
                    String::from_utf8_lossy(&buffer[..bytes_read])
                );

                if let Err(error) = socket.write_all(&buffer[..bytes_read]).await {
                    eprintln!("Write error: {error}");
                    return;
                }
            }
        });
    }
}
