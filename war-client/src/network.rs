use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};

use iced::{Subscription, stream};

use tokio::net::TcpStream;

use war_protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage};

#[derive(Debug, Clone)]
pub enum Command {
    Connect { address: String, username: String },

    Send(ClientMessage),

    Disconnect,
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(mpsc::Sender<Command>),

    Connected,

    ServerMessage(ServerMessage),

    Disconnected(String),
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Event> {
    stream::channel(100, async |mut output| {
        let (command_sender, mut commands) = mpsc::channel(100);

        let _ = output.send(Event::Ready(command_sender)).await;

        loop {
            let Some(command) = commands.next().await else {
                return;
            };

            let Command::Connect { address, username } = command else {
                continue;
            };

            match TcpStream::connect(&address).await {
                Ok(stream) => {
                    run_connection(stream, username, &mut commands, &mut output).await;
                }

                Err(error) => {
                    let _ = output.send(Event::Disconnected(error.to_string())).await;
                }
            }
        }
    })
}

async fn run_connection(
    stream: TcpStream,
    username: String,
    commands: &mut mpsc::Receiver<Command>,
    output: &mut mpsc::Sender<Event>,
) {
    let (mut reader, mut writer) = stream.into_split();

    let hello = ClientMessage::Hello {
        protocol_version: PROTOCOL_VERSION,

        username,
    };

    if write_message(&mut writer, &hello).await.is_err() {
        return;
    }

    let _ = output.send(Event::Connected).await;

    loop {
        tokio::select! {
            incoming =
                read_message::<_, ServerMessage>(
                    &mut reader
                )
            => {
                match incoming {
                    Ok(message) => {
                        let _ =
                            output
                                .send(
                                    Event::ServerMessage(
                                        message
                                    )
                                )
                                .await;
                    }

                    Err(error) => {
                        let _ =
                            output
                                .send(
                                    Event::Disconnected(
                                        error.to_string()
                                    )
                                )
                                .await;

                        return;
                    }
                }
            }

            command =
                commands.next()
            => {
                match command {
                    Some(
                        Command::Send(message)
                    ) => {
                        if write_message(
                            &mut writer,
                            &message
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                    }

                    Some(
                        Command::Disconnect
                    ) |
                    None => {
                        return;
                    }

                    Some(
                        Command::Connect { .. }
                    ) => {
                        // Already connected.
                    }
                }
            }
        }
    }
}

mod network;

use iced::futures::channel::mpsc;

use iced::{Element, Subscription};

use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};

use war_protocol::{
    PlayerGameView, RoomId, RoomSummary, RoomView, RoundOutcome, RoundResult, ServerMessage,
};

struct App {
    server_address: String,
    username: String,

    connected: bool,

    network: Option<mpsc::Sender<network::Command>>,

    status: String,

    rooms: Vec<RoomSummary>,

    room: Option<RoomView>,

    game: Option<PlayerGameView>,

    last_round: Option<RoundResult>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            server_address: "127.0.0.1:9001".into(),

            username: String::new(),

            connected: false,

            network: None,

            status: "Disconnected".into(),

            rooms: Vec::new(),

            room: None,

            game: None,

            last_round: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ServerAddressChanged(String),
    UsernameChanged(String),

    Connect,

    CreateRoom,
    JoinRoom(RoomId),
    LeaveRoom,

    PlayCard,

    Network(network::Event),
}

impl App {
    fn send_network(&mut self, command: network::Command) {
        if let Some(sender) = self.network.as_mut() {
            let _ = sender.try_send(command);
        }
    }
}

fn update(app: &mut App, message: Message) {
    match message {
        Message::ServerAddressChanged(address) => {
            app.server_address = address;
        }

        Message::UsernameChanged(username) => {
            app.username = username;
        }

        Message::Connect => {
            app.status = "Connecting...".into();

            app.send_network(network::Command::Connect {
                address: app.server_address.clone(),

                username: app.username.clone(),
            });
        }

        Message::CreateRoom => {
            app.send_network(network::Command::Send(
                war_protocol::ClientMessage::CreateRoom,
            ));
        }

        Message::JoinRoom(room_id) => {
            app.send_network(network::Command::Send(
                war_protocol::ClientMessage::JoinRoom { room_id },
            ));
        }

        Message::LeaveRoom => {
            app.send_network(network::Command::Send(
                war_protocol::ClientMessage::LeaveRoom,
            ));
        }

        Message::PlayCard => {
            app.send_network(network::Command::Send(
                war_protocol::ClientMessage::PlayCard,
            ));
        }

        Message::Network(event) => {
            handle_network_event(app, event);
        }
    }
}

fn handle_network_event(app: &mut App, event: network::Event) {
    match event {
        network::Event::Ready(sender) => {
            app.network = Some(sender);
        }

        network::Event::Connected => {
            app.status = "Connected".into();
        }

        network::Event::Disconnected(reason) => {
            app.connected = false;

            app.status = format!("Disconnected: {reason}");

            app.room = None;
            app.game = None;
        }

        network::Event::ServerMessage(message) => {
            handle_server_message(app, message);
        }
    }
}

fn handle_server_message(app: &mut App, message: ServerMessage) {
    match message {
        ServerMessage::Welcome { .. } => {
            app.connected = true;
            app.status = "Connected".into();
        }

        ServerMessage::RoomList { rooms } => {
            app.rooms = rooms;
        }

        ServerMessage::RoomState { room } => {
            app.room = Some(room);
        }

        ServerMessage::GameState { game } => {
            app.game = Some(game);
        }

        ServerMessage::RoundResult { result } => {
            app.last_round = Some(result);
        }

        ServerMessage::LeftRoom => {
            app.room = None;
            app.game = None;
            app.last_round = None;
        }

        ServerMessage::Error { message } => {
            app.status = message;
        }

        ServerMessage::Pong { .. } => {}
    }
}

fn subscription(_app: &App) -> Subscription<Message> {
    network::subscription().map(Message::Network)
}

fn connect_view(app: &App) -> Element<'_, Message> {
    let content = column![
        text("WAR").size(48),
        text("Server"),
        text_input("127.0.0.1:9001", &app.server_address,).on_input(Message::ServerAddressChanged),
        text("Username"),
        text_input("Alice", &app.username,).on_input(Message::UsernameChanged),
        button("CONNECT").on_press(Message::Connect),
        text(&app.status),
    ]
    .spacing(12)
    .padding(30);

    container(content)
        .center_x(iced::Fill)
        .center_y(iced::Fill)
        .into()
}

fn lobby_view(app: &App) -> Element<'_, Message> {
    let mut room_column = Column::new().spacing(10);

    for room_info in &app.rooms {
        let description = format!(
            "Room #{} — {} — {}/2",
            room_info.id, room_info.host_name, room_info.player_count,
        );

        let join = button("JOIN").on_press_maybe(
            (room_info.player_count == 1 && !room_info.in_game)
                .then_some(Message::JoinRoom(room_info.id)),
        );

        room_column = room_column.push(row![text(description), join,].spacing(20));
    }

    column![
        text(format!("Connected as {}", app.username)),
        button("CREATE ROOM").on_press(Message::CreateRoom),
        text("Available Games").size(28),
        scrollable(room_column),
        text(&app.status),
    ]
    .spacing(15)
    .padding(30)
    .into()
}

fn room_view(app: &App, room: &RoomView) -> Element<'_, Message> {
    let guest = room
        .guest_name
        .as_deref()
        .unwrap_or("Waiting for opponent...");

    column![
        text(format!("Room #{}", room.id)).size(36),
        text(format!("Host: {}", room.host_name)),
        text(format!("Opponent: {guest}")),
        button("LEAVE ROOM").on_press(Message::LeaveRoom),
    ]
    .spacing(15)
    .padding(30)
    .into()
}
