use std::io;

use tokio::net::{
    TcpListener, TcpStream,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};
use tokio::sync::mpsc;
mod game;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use war_protocol::wire;
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
        if let Some(client) = self.clients.remove(&client_id)
            && let Some(room_id) = client.room_id
        {
            self.remove_from_room(client_id, room_id);
            self.broadcast_rooms();
        }
    }
}

impl Server {
    fn send(&self, client_id: ClientId, message: ServerMessage) {
        if let Some(client) = self.clients.get(&client_id) {
            let _ = client.sender.send(message);
        }
    }

    fn leave_room(&mut self, client_id: ClientId) {
        let Some(room_id) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.room_id.take())
        else {
            return;
        };

        self.remove_from_room(client_id, room_id);
        self.send(client_id, ServerMessage::LeftRoom);
        self.broadcast_rooms();
    }

    fn remove_from_room(&mut self, client_id: ClientId, room_id: RoomId) {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };

        if room.host == client_id {
            let guest = room.guest;
            self.rooms.remove(&room_id);

            if let Some(guest_id) = guest {
                if let Some(client) = self.clients.get_mut(&guest_id) {
                    client.room_id = None;
                }

                self.send(guest_id, ServerMessage::LeftRoom);
            }
        } else if room.guest == Some(client_id) {
            room.guest = None;
            room.game = None;
            self.send_room_state(room_id);
        }
    }
}

impl Server {
    fn send_error(&self, client_id: ClientId, message: &str) {
        self.send(
            client_id,
            ServerMessage::Error {
                message: message.into(),
            },
        );
    }

    fn handle_message(&mut self, client_id: ClientId, message: ClientMessage) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };

        if client.username.is_none()
            && !matches!(
                message,
                ClientMessage::Hello { .. } | ClientMessage::Ping { .. }
            )
        {
            self.send_error(client_id, "Log in before joining or playing a game.");
            return;
        }

        match message {
            ClientMessage::Hello {
                protocol_version,
                username,
            } => {
                if client.username.is_some() {
                    self.send_error(client_id, "You are already logged in.");
                } else {
                    self.login(client_id, protocol_version, username);
                }
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
            self.send_error(client_id, "Protocol version mismatch.");
            return;
        }

        let username = username.trim().to_string();

        if username.is_empty() || username.len() > 20 {
            self.send_error(client_id, "Invalid username.");
            return;
        }

        let already_used = self
            .clients
            .values()
            .any(|client| client.username.as_deref() == Some(&username));

        if already_used {
            self.send_error(client_id, "That username is already in use.");
            return;
        }

        if let Some(client) = self.clients.get_mut(&client_id) {
            client.username = Some(username);
        }

        self.send(client_id, ServerMessage::Welcome { client_id });

        self.send_rooms(client_id);
    }
}

impl Server {
    fn room_summaries(&self) -> Vec<RoomSummary> {
        let mut rooms: Vec<_> = self
            .rooms
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
            .collect();
        rooms.sort_unstable_by_key(|room| room.id);
        rooms
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
        let Some(client) = self.clients.get_mut(&client_id) else {
            return;
        };

        if client.room_id.is_some() {
            self.send_error(client_id, "Leave your current room first.");
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

        client.room_id = Some(room_id);

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
            self.send_error(client_id, "Leave your current room first.");
            return;
        }

        let Some(guest_name) = client.username.clone() else {
            self.send_error(client_id, "Log in before joining a game.");
            return;
        };

        let Some(room) = self.rooms.get_mut(&room_id) else {
            self.send_error(client_id, "Room does not exist.");
            return;
        };

        if room.guest.is_some() {
            self.send_error(client_id, "Room is full.");
            return;
        }

        let host_id = room.host;
        let Some(host_name) = self
            .clients
            .get(&host_id)
            .and_then(|host| host.username.clone())
        else {
            self.send_error(client_id, "Room host is no longer available.");
            return;
        };

        room.guest = Some(client_id);
        room.game = Some(Game::new(
            room_id, host_id, host_name, client_id, guest_name,
        ));
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
            self.send_error(client_id, "Join a game before playing a card.");
            return;
        };

        let resolution = {
            let Some(game) = self
                .rooms
                .get_mut(&room_id)
                .and_then(|room| room.game.as_mut())
            else {
                self.send_error(client_id, "There is no active game in this room.");
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
    events: &mpsc::UnboundedSender<ServerEvent>,
) -> io::Result<()> {
    loop {
        let message = wire::read_message(&mut reader).await?;
        if events
            .send(ServerEvent::Message { client_id, message })
            .is_err()
        {
            return Ok(());
        }
    }
}

async fn writer_loop(
    mut writer: OwnedWriteHalf,
    mut messages: mpsc::UnboundedReceiver<ServerMessage>,
) -> io::Result<()> {
    while let Some(message) = messages.recv().await {
        wire::write_message(&mut writer, &message).await?;
    }
    Ok(())
}

async fn handle_connection(
    socket: TcpStream,
    client_id: ClientId,
    events: mpsc::UnboundedSender<ServerEvent>,
) {
    let (reader, writer) = socket.into_split();
    let (sender, messages) = mpsc::unbounded_channel();

    if events
        .send(ServerEvent::Connected { client_id, sender })
        .is_err()
    {
        return;
    }

    let result = tokio::select! {
        result = reader_loop(client_id, reader, &events) => result,
        result = writer_loop(writer, messages) => result,
    };

    if let Err(error) = result
        && error.kind() != io::ErrorKind::UnexpectedEof
        && error.kind() != io::ErrorKind::ConnectionReset
    {
        eprintln!("Client {client_id} disconnected: {error}");
    }

    let _ = events.send(ServerEvent::Disconnected { client_id });
}

async fn accept_loop(
    listener: TcpListener,
    events: mpsc::UnboundedSender<ServerEvent>,
) -> std::io::Result<()> {
    loop {
        let (socket, address) = listener.accept().await?;

        let client_id = NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);

        println!("Client {client_id} connected: {address}");

        tokio::spawn(handle_connection(socket, client_id, events.clone()));
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:9001").await?;

    println!("WAR server listening on 127.0.0.1:9001");

    serve(listener).await
}

async fn serve(listener: TcpListener) -> io::Result<()> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::select! {
        result = accept_loop(listener, event_tx) => result,
        () = Server::new().run(event_rx) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn send(stream: &mut TcpStream, message: ClientMessage) {
        wire::write_message(stream, &message).await.unwrap();
    }

    async fn receive(stream: &mut TcpStream) -> ServerMessage {
        wire::read_message(stream).await.unwrap()
    }

    async fn login(stream: &mut TcpStream, username: &str) {
        send(
            stream,
            ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                username: username.into(),
            },
        )
        .await;
        assert!(matches!(
            receive(stream).await,
            ServerMessage::Welcome { .. }
        ));
        assert!(matches!(
            receive(stream).await,
            ServerMessage::RoomList { .. }
        ));
    }

    async fn game_state(stream: &mut TcpStream) -> war_protocol::PlayerGameView {
        match receive(stream).await {
            ServerMessage::GameState { game } => game,
            message => panic!("Expected game state, got {message:?}"),
        }
    }

    async fn join_game(host: &mut TcpStream, guest: &mut TcpStream, room_id: RoomId) {
        send(guest, ClientMessage::JoinRoom { room_id }).await;
        for player in [host, guest] {
            assert!(matches!(
                receive(player).await,
                ServerMessage::RoomState { room } if room.id == room_id && room.guest_name.is_some()
            ));
            let game = game_state(player).await;
            assert_eq!(game.your_cards_remaining, 26);
            assert_eq!(game.your_score, 0);
            assert!(matches!(
                receive(player).await,
                ServerMessage::RoomList { .. }
            ));
        }
    }

    #[tokio::test]
    async fn play_full_game_and_recover_from_departures() {
        tokio::time::timeout(Duration::from_secs(10), async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(serve(listener));

            let mut host = TcpStream::connect(address).await.unwrap();
            login(&mut host, "Host").await;
            send(&mut host, ClientMessage::CreateRoom).await;
            let room_id = match receive(&mut host).await {
                ServerMessage::RoomState { room } => room.id,
                message => panic!("Expected room state, got {message:?}"),
            };
            assert!(matches!(
                receive(&mut host).await,
                ServerMessage::RoomList { .. }
            ));

            let mut guest = TcpStream::connect(address).await.unwrap();
            send(&mut guest, ClientMessage::JoinRoom { room_id }).await;
            assert!(matches!(
                receive(&mut guest).await,
                ServerMessage::Error { .. }
            ));
            login(&mut guest, "Guest").await;
            join_game(&mut host, &mut guest, room_id).await;

            for remaining in (0..26).rev() {
                send(&mut host, ClientMessage::PlayCard).await;
                let waiting = game_state(&mut host).await;
                assert!(waiting.you_played);
                assert_eq!(waiting.your_cards_remaining, remaining);
                assert!(game_state(&mut guest).await.opponent_played);

                send(&mut guest, ClientMessage::PlayCard).await;
                for player in [&mut host, &mut guest] {
                    assert!(matches!(
                        receive(player).await,
                        ServerMessage::RoundResult { .. }
                    ));
                    let game = game_state(player).await;
                    assert_eq!(game.your_cards_remaining, remaining);
                    assert_eq!(game.opponent_cards_remaining, remaining);
                    assert_eq!(game.finished, remaining == 0);
                    assert!(!game.you_played && !game.opponent_played);
                    assert_eq!(
                        game.your_score + game.opponent_score + game.battle_pot,
                        52 - 2 * remaining
                    );
                    if game.finished {
                        assert_eq!(game.battle_pot, 0);
                    }
                }
            }

            drop(guest);
            assert!(matches!(
                receive(&mut host).await,
                ServerMessage::RoomState { room } if room.id == room_id && room.guest_name.is_none()
            ));
            assert!(matches!(
                receive(&mut host).await,
                ServerMessage::RoomList { rooms }
                    if rooms.len() == 1 && rooms[0].player_count == 1 && !rooms[0].in_game
            ));

            let mut guest = TcpStream::connect(address).await.unwrap();
            login(&mut guest, "Guest").await;
            join_game(&mut host, &mut guest, room_id).await;
            send(&mut guest, ClientMessage::LeaveRoom).await;
            assert!(matches!(
                receive(&mut host).await,
                ServerMessage::RoomState { room } if room.guest_name.is_none()
            ));
            assert!(matches!(receive(&mut guest).await, ServerMessage::LeftRoom));
            for player in [&mut host, &mut guest] {
                assert!(matches!(
                    receive(player).await,
                    ServerMessage::RoomList { .. }
                ));
            }
            join_game(&mut host, &mut guest, room_id).await;

            drop(host);
            assert!(matches!(receive(&mut guest).await, ServerMessage::LeftRoom));
            assert!(matches!(
                receive(&mut guest).await,
                ServerMessage::RoomList { rooms } if rooms.is_empty()
            ));

            send(&mut guest, ClientMessage::CreateRoom).await;
            assert!(matches!(
                receive(&mut guest).await,
                ServerMessage::RoomState { .. }
            ));
            assert!(matches!(
                receive(&mut guest).await,
                ServerMessage::RoomList { .. }
            ));
            server.abort();
            assert!(server.await.unwrap_err().is_cancelled());
        })
        .await
        .expect("game traffic stalled");
    }
}
