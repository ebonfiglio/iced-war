use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{
    TcpListener,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};
use tokio::sync::mpsc;
mod game;
mod wire;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use war_protocol::{
    ClientId, ClientMessage, PROTOCOL_VERSION, RoomId, RoomSummary, RoomView, ServerMessage,
};

use game::Game;

static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

enum ServerEvent {
    Connected {
        client_id: ClientId,
        sender: mpsc::UnboundedSender<ServerMessage>,
    },

    Message {
        client_id: ClientId,
        message: ClientMessage,
    },

    Disconnected {
        client_id: ClientId,
    },
}

struct Client {
    username: Option<String>,

    sender: mpsc::UnboundedSender<ServerMessage>,

    room_id: Option<RoomId>,
}

struct Room {
    id: RoomId,

    host: ClientId,

    guest: Option<ClientId>,

    game: Option<Game>,
}

struct Server {
    clients: HashMap<ClientId, Client>,
    rooms: HashMap<RoomId, Room>,

    next_room_id: RoomId,
}

impl Server {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            rooms: HashMap::new(),

            next_room_id: 1,
        }
    }
}

impl Server {
    async fn run(mut self, mut events: mpsc::UnboundedReceiver<ServerEvent>) {
        while let Some(event) = events.recv().await {
            match event {
                ServerEvent::Connected { client_id, sender } => {
                    self.clients.insert(
                        client_id,
                        Client {
                            username: None,
                            sender,
                            room_id: None,
                        },
                    );
                }

                ServerEvent::Message { client_id, message } => {
                    self.handle_message(client_id, message);
                }

                ServerEvent::Disconnected { client_id } => {
                    self.remove_player(client_id);
                }
            }
        }
    }
}

impl Server {
    fn remove_player(&mut self, client_id: ClientId) {
        self.leave_room(client_id);
        self.clients.remove(&client_id);
    }
}

impl Server {
    fn send(&self, client_id: ClientId, message: ServerMessage) {
        if let Some(client) = self.clients.get(&client_id) {
            let _ = client.sender.send(message);
        }
    }

    fn leave_room(&mut self, client_id: ClientId) {
        let room_id = self
            .clients
            .get(&client_id)
            .and_then(|client| client.room_id);

        let Some(room_id) = room_id else {
            return;
        };

        let Some(room) = self.rooms.get(&room_id) else {
            return;
        };

        let is_host = room.host == client_id;

        let guest = room.guest;

        if is_host {
            self.rooms.remove(&room_id);

            if let Some(guest_id) = guest {
                if let Some(client) = self.clients.get_mut(&guest_id) {
                    client.room_id = None;
                }

                self.send(guest_id, ServerMessage::LeftRoom);
            }
        } else {
            if let Some(room) = self.rooms.get_mut(&room_id) {
                room.guest = None;
                room.game = None;
            }

            self.send_room_state(room_id);
        }

        if let Some(client) = self.clients.get_mut(&client_id) {
            client.room_id = None;
        }

        self.send(client_id, ServerMessage::LeftRoom);

        self.broadcast_rooms();
    }
}

impl Server {
    fn handle_message(&mut self, client_id: ClientId, message: ClientMessage) {
        match message {
            ClientMessage::Hello {
                protocol_version,
                username,
            } => {
                self.login(client_id, protocol_version, username);
            }

            ClientMessage::RequestRooms => {
                self.send_rooms(client_id);
            }

            ClientMessage::CreateRoom => {
                self.create_room(client_id);
            }

            ClientMessage::JoinRoom { room_id } => {
                self.join_room(client_id, room_id);
            }

            ClientMessage::LeaveRoom => {
                self.leave_room(client_id);
            }

            ClientMessage::PlayCard => {
                self.play_card(client_id);
            }

            ClientMessage::Ping { nonce } => {
                self.send(client_id, ServerMessage::Pong { nonce });
            }
        }
    }
}

impl Server {
    fn login(&mut self, client_id: ClientId, version: u32, username: String) {
        if version != PROTOCOL_VERSION {
            self.send(
                client_id,
                ServerMessage::Error {
                    message: "Protocol version mismatch.".into(),
                },
            );

            return;
        }

        let username = username.trim().to_string();

        if username.is_empty() || username.len() > 20 {
            self.send(
                client_id,
                ServerMessage::Error {
                    message: "Invalid username.".into(),
                },
            );

            return;
        }

        let already_used = self
            .clients
            .values()
            .any(|client| client.username.as_deref() == Some(&username));

        if already_used {
            self.send(
                client_id,
                ServerMessage::Error {
                    message: "That username is already in use.".into(),
                },
            );

            return;
        }

        if let Some(client) = self.clients.get_mut(&client_id) {
            client.username = Some(username);
        }

        self.send(client_id, ServerMessage::Welcome { client_id });

        self.broadcast_rooms();
    }
}

impl Server {
    fn room_summaries(&self) -> Vec<RoomSummary> {
        self.rooms
            .values()
            .filter_map(|room| {
                let host = self.clients.get(&room.host)?;

                let host_name = host.username.clone()?;

                Some(RoomSummary {
                    id: room.id,

                    host_name,

                    player_count: if room.guest.is_some() { 2 } else { 1 },

                    in_game: room.game.is_some(),
                })
            })
            .collect()
    }
}

impl Server {
    fn send_rooms(&self, client_id: ClientId) {
        self.send(
            client_id,
            ServerMessage::RoomList {
                rooms: self.room_summaries(),
            },
        );
    }

    fn broadcast_rooms(&self) {
        let message = ServerMessage::RoomList {
            rooms: self.room_summaries(),
        };

        for client in self.clients.values() {
            if client.username.is_some() {
                let _ = client.sender.send(message.clone());
            }
        }
    }
}

impl Server {
    fn create_room(&mut self, client_id: ClientId) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };

        if client.username.is_none() {
            return;
        }

        if client.room_id.is_some() {
            self.send(
                client_id,
                ServerMessage::Error {
                    message: "Leave your current room first.".into(),
                },
            );

            return;
        }

        let room_id = self.next_room_id;
        self.next_room_id += 1;

        self.rooms.insert(
            room_id,
            Room {
                id: room_id,
                host: client_id,
                guest: None,
                game: None,
            },
        );

        self.clients.get_mut(&client_id).unwrap().room_id = Some(room_id);

        self.send_room_state(room_id);
        self.broadcast_rooms();
    }
}

impl Server {
    fn room_view(&self, room_id: RoomId) -> Option<RoomView> {
        let room = self.rooms.get(&room_id)?;

        let host_name = self.clients.get(&room.host)?.username.clone()?;

        let guest_name = room
            .guest
            .and_then(|id| self.clients.get(&id)?.username.clone());

        Some(RoomView {
            id: room.id,
            host_name,
            guest_name,
        })
    }

    fn send_room_state(&self, room_id: RoomId) {
        let Some(view) = self.room_view(room_id) else {
            return;
        };

        let Some(room) = self.rooms.get(&room_id) else {
            return;
        };

        self.send(room.host, ServerMessage::RoomState { room: view.clone() });

        if let Some(guest) = room.guest {
            self.send(guest, ServerMessage::RoomState { room: view });
        }
    }
}

impl Server {
    fn join_room(&mut self, client_id: ClientId, room_id: RoomId) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };

        if client.room_id.is_some() {
            return;
        }

        let Some(room) = self.rooms.get(&room_id) else {
            self.send(
                client_id,
                ServerMessage::Error {
                    message: "Room does not exist.".into(),
                },
            );

            return;
        };

        if room.host == client_id || room.guest.is_some() {
            return;
        }

        let host_id = room.host;

        let host_name = self.clients[&host_id].username.clone().unwrap();

        let guest_name = self.clients[&client_id].username.clone().unwrap();

        let game = Game::new(room_id, host_id, host_name, client_id, guest_name);

        {
            let room = self.rooms.get_mut(&room_id).unwrap();

            room.guest = Some(client_id);

            room.game = Some(game);
        }

        self.clients.get_mut(&client_id).unwrap().room_id = Some(room_id);

        self.send_room_state(room_id);
        self.send_game_states(room_id);
        self.broadcast_rooms();
    }
}

impl Server {
    fn send_game_states(&self, room_id: RoomId) {
        let Some(room) = self.rooms.get(&room_id) else {
            return;
        };

        let Some(game) = room.game.as_ref() else {
            return;
        };

        let player1 = room.host;

        let Some(player2) = room.guest else {
            return;
        };

        if let Some(view) = game.view_for(player1) {
            self.send(player1, ServerMessage::GameState { game: view });
        }

        if let Some(view) = game.view_for(player2) {
            self.send(player2, ServerMessage::GameState { game: view });
        }
    }
}

impl Server {
    fn play_card(&mut self, client_id: ClientId) {
        let Some(room_id) = self
            .clients
            .get(&client_id)
            .and_then(|client| client.room_id)
        else {
            return;
        };

        let resolution = {
            let Some(room) = self.rooms.get_mut(&room_id) else {
                return;
            };

            let Some(game) = room.game.as_mut() else {
                return;
            };

            match game.play_card(client_id) {
                Ok(resolution) => resolution,

                Err(message) => {
                    self.send(client_id, ServerMessage::Error { message });

                    return;
                }
            }
        };

        if let Some(resolution) = resolution {
            self.send_round_results(room_id, &resolution);
        }

        self.send_game_states(room_id);
    }
}

impl Server {
    fn send_round_results(&self, room_id: RoomId, resolution: &game::Resolution) {
        let Some(room) = self.rooms.get(&room_id) else {
            return;
        };

        let Some(game) = room.game.as_ref() else {
            return;
        };

        let player1 = room.host;

        let Some(player2) = room.guest else {
            return;
        };

        if let Some(result) = game.result_for(resolution, player1) {
            self.send(player1, ServerMessage::RoundResult { result });
        }

        if let Some(result) = game.result_for(resolution, player2) {
            self.send(player2, ServerMessage::RoundResult { result });
        }
    }
}

async fn reader_loop(
    client_id: ClientId,
    mut reader: OwnedReadHalf,
    events: mpsc::UnboundedSender<ServerEvent>,
) {
    loop {
        match wire::read_message::<_, ClientMessage>(&mut reader).await {
            Ok(message) => {
                let _ = events.send(ServerEvent::Message { client_id, message });
            }

            Err(_) => {
                let _ = events.send(ServerEvent::Disconnected { client_id });

                return;
            }
        }
    }
}

async fn writer_loop(
    mut writer: OwnedWriteHalf,
    mut messages: mpsc::UnboundedReceiver<ServerMessage>,
) {
    while let Some(message) = messages.recv().await {
        if wire::write_message(&mut writer, &message).await.is_err() {
            return;
        }
    }
}

async fn accept_loop(
    listener: TcpListener,
    events: mpsc::UnboundedSender<ServerEvent>,
) -> std::io::Result<()> {
    loop {
        let (socket, address) = listener.accept().await?;

        let client_id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);

        println!("Client {client_id} connected: {address}");

        let (reader, writer) = socket.into_split();

        let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();

        let _ = events.send(ServerEvent::Connected {
            client_id,
            sender: outgoing_tx,
        });

        tokio::spawn(reader_loop(client_id, reader, events.clone()));

        tokio::spawn(writer_loop(writer, outgoing_rx));
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:9001").await?;

    println!("WAR server listening on 127.0.0.1:9001");

    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(accept_loop(listener, event_tx));

    Server::new().run(event_rx).await;

    Ok(())
}
