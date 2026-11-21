use std::io;
use std::time::Duration;

use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};
use iced::{Subscription, stream};
use tokio::net::TcpStream;
use tokio::time::timeout;
use war_protocol::wire::{read_message, write_message};
use war_protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage};

#[derive(Debug, Clone)]
pub enum Command {
    Connect { address: String, username: String },
    Send(ClientMessage),
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(mpsc::Sender<Command>),
    ServerMessage(ServerMessage),
    Disconnected(String),
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Event> {
    stream::channel(100, async |mut output| {
        let (command_sender, mut commands) = mpsc::channel(100);
        if output.send(Event::Ready(command_sender)).await.is_err() {
            return;
        }

        while let Some(command) = commands.next().await {
            let Command::Connect { address, username } = command else {
                continue;
            };

            if let Err(error) = run_connection(&address, username, &mut commands, &mut output).await
                && output
                    .send(Event::Disconnected(error.to_string()))
                    .await
                    .is_err()
            {
                return;
            }
        }
    })
}

async fn run_connection(
    address: &str,
    username: String,
    commands: &mut mpsc::Receiver<Command>,
    output: &mut mpsc::Sender<Event>,
) -> io::Result<()> {
    let (stream, welcome) = timeout(Duration::from_secs(10), async {
        let mut stream = TcpStream::connect(address).await?;
        write_message(
            &mut stream,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username,
            },
        )
        .await?;

        match read_message(&mut stream).await? {
            welcome @ ServerMessage::Welcome { .. } => Ok((stream, welcome)),
            ServerMessage::Error { message } => {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, message))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Expected a login response from the server.",
            )),
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Connection or login timed out."))??;

    if output.send(Event::ServerMessage(welcome)).await.is_err() {
        return Ok(());
    }

    let (mut reader, mut writer) = stream.into_split();

    // Keep a partially read frame alive while outgoing commands are sent.
    tokio::select! {
        result = async {
            loop {
                let message = read_message(&mut reader).await?;
                if output.send(Event::ServerMessage(message)).await.is_err() {
                    return Ok(());
                }
            }
        } => result,
        result = async {
            while let Some(command) = commands.next().await {
                if let Command::Send(message) = command {
                    write_message(&mut writer, &message).await?;
                }
            }
            Ok(())
        } => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn can_retry_rejected_login_and_read_while_sending() {
        timeout(Duration::from_secs(5), async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap().to_string();

            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(matches!(
                    read_message(&mut stream).await.unwrap(),
                    ClientMessage::Hello { username, .. } if username == "Taken"
                ));
                write_message(
                    &mut stream,
                    &ServerMessage::Error {
                        message: "That username is already in use.".into(),
                    },
                )
                .await
                .unwrap();
                drop(stream);

                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(matches!(
                    read_message(&mut stream).await.unwrap(),
                    ClientMessage::Hello { username, .. } if username == "Available"
                ));
                write_message(&mut stream, &ServerMessage::Welcome { client_id: 1 })
                    .await
                    .unwrap();

                let payload = war_protocol::encode(&ServerMessage::Pong { nonce: 42 }).unwrap();
                let length = (payload.len() as u32).to_be_bytes();
                stream.write_all(&length[..2]).await.unwrap();
                assert!(matches!(
                    read_message(&mut stream).await.unwrap(),
                    ClientMessage::Ping { nonce: 42 }
                ));
                stream.write_all(&length[2..]).await.unwrap();
                stream.write_all(&payload).await.unwrap();
            });

            let events = worker();
            tokio::pin!(events);
            let Some(Event::Ready(mut commands)) = events.next().await else {
                panic!("Expected the network worker to start");
            };
            commands
                .send(Command::Connect {
                    address: address.clone(),
                    username: "Taken".into(),
                })
                .await
                .unwrap();
            assert!(matches!(
                events.next().await,
                Some(Event::Disconnected(reason)) if reason == "That username is already in use."
            ));
            commands
                .send(Command::Connect {
                    address,
                    username: "Available".into(),
                })
                .await
                .unwrap();
            assert!(matches!(
                events.next().await,
                Some(Event::ServerMessage(ServerMessage::Welcome {
                    client_id: 1
                }))
            ));
            commands
                .send(Command::Send(ClientMessage::Ping { nonce: 42 }))
                .await
                .unwrap();
            assert!(matches!(
                events.next().await,
                Some(Event::ServerMessage(ServerMessage::Pong { nonce: 42 }))
            ));
            assert!(matches!(events.next().await, Some(Event::Disconnected(_))));
            server.await.unwrap();
        })
        .await
        .expect("test connection stalled");
    }
}
