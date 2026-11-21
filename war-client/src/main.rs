mod network;
mod table;

use std::cmp::Ordering;

use iced::futures::channel::mpsc;

use iced::{Element, Subscription};

use iced::widget::{
    Canvas, Column, button, column, container, row, scrollable, space, text, text_input,
};

use war_protocol::{
    ClientMessage, PlayerGameView, RoomId, RoomSummary, RoomView, RoundOutcome, RoundResult,
    ServerMessage,
};

#[derive(PartialEq, Eq)]
enum Connection {
    Disconnected,
    Connecting,
    Connected,
}

struct App {
    server_address: String,
    username: String,

    connection: Connection,

    network: Option<mpsc::Sender<network::Command>>,

    status: String,

    rooms: Vec<RoomSummary>,

    room: Option<RoomView>,

    game: Option<PlayerGameView>,

    last_round: Option<RoundResult>,

    play_pending: bool,

    table_art: table::TableArt,
}

impl Default for App {
    fn default() -> Self {
        Self {
            server_address: "127.0.0.1:9001".into(),

            username: String::new(),

            connection: Connection::Disconnected,

            network: None,

            status: "Disconnected".into(),

            rooms: Vec::new(),

            room: None,

            game: None,

            last_round: None,

            play_pending: false,

            table_art: table::TableArt::default(),
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
    fn send_network(&mut self, command: network::Command) -> bool {
        match self.network.as_mut() {
            Some(sender) => match sender.try_send(command) {
                Ok(()) => return true,
                Err(error) => self.status = format!("Could not send command: {error}"),
            },
            None => self.status = "Network is not ready.".into(),
        }

        false
    }

    fn clear_game(&mut self) {
        self.game = None;
        self.last_round = None;
        self.play_pending = false;
    }

    fn can_play(&self, game: &PlayerGameView) -> bool {
        !self.play_pending && !game.finished && !game.you_played && game.your_cards_remaining > 0
    }
}

fn update(app: &mut App, message: Message) {
    match message {
        Message::ServerAddressChanged(address) => {
            if app.connection == Connection::Disconnected {
                app.server_address = address;
            }
        }

        Message::UsernameChanged(username) => {
            if app.connection == Connection::Disconnected {
                app.username = username;
            }
        }

        Message::Connect => {
            if app.connection == Connection::Disconnected {
                app.server_address = app.server_address.trim().to_string();
                app.username = app.username.trim().to_string();

                if app.send_network(network::Command::Connect {
                    address: app.server_address.clone(),
                    username: app.username.clone(),
                }) {
                    app.connection = Connection::Connecting;
                    app.status = "Connecting...".into();
                }
            }
        }

        Message::CreateRoom => {
            app.send_network(network::Command::Send(ClientMessage::CreateRoom));
        }

        Message::JoinRoom(room_id) => {
            app.send_network(network::Command::Send(ClientMessage::JoinRoom { room_id }));
        }

        Message::LeaveRoom => {
            app.send_network(network::Command::Send(ClientMessage::LeaveRoom));
        }

        Message::PlayCard => {
            if app.game.as_ref().is_some_and(|game| app.can_play(game)) {
                app.play_pending =
                    app.send_network(network::Command::Send(ClientMessage::PlayCard));
            }
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

        network::Event::Disconnected(reason) => {
            app.status = format!("Disconnected: {reason}");
            app.connection = Connection::Disconnected;

            app.rooms.clear();
            app.room = None;
            app.clear_game();
        }

        network::Event::ServerMessage(message) => {
            handle_server_message(app, message);
        }
    }
}

fn handle_server_message(app: &mut App, message: ServerMessage) {
    match message {
        ServerMessage::Welcome { .. } => {
            app.connection = Connection::Connected;
            app.status = "Connected".into();
        }

        ServerMessage::RoomList { rooms } => {
            app.rooms = rooms;
        }

        ServerMessage::RoomState { room } => {
            if room.guest_name.is_none() && app.game.is_some() {
                app.clear_game();
                app.status = "Opponent left the room.".into();
            } else {
                app.status = "Connected".into();
            }
            app.room = Some(room);
        }

        ServerMessage::GameState { game } => {
            if app
                .game
                .as_ref()
                .is_some_and(|previous| game.your_cards_remaining < previous.your_cards_remaining)
            {
                app.play_pending = false;
                app.status = "Connected".into();
            }
            app.game = Some(game);
        }

        ServerMessage::RoundResult { result } => {
            app.last_round = Some(result);
        }

        ServerMessage::LeftRoom => {
            app.room = None;
            app.clear_game();
            app.status = "Left room.".into();
        }

        ServerMessage::Error { message } => {
            app.play_pending = false;
            app.status = message;
        }

        ServerMessage::Pong { .. } => {}
    }
}

fn subscription(_app: &App) -> Subscription<Message> {
    network::subscription().map(Message::Network)
}

fn connect_view(app: &App) -> Element<'_, Message> {
    let can_edit = app.connection == Connection::Disconnected;

    let content = column![
        text("WAR").size(48),
        text("Server"),
        text_input("127.0.0.1:9001", &app.server_address)
            .on_input_maybe(can_edit.then_some(Message::ServerAddressChanged)),
        text("Username"),
        text_input("Alice", &app.username)
            .on_input_maybe(can_edit.then_some(Message::UsernameChanged)),
        button("CONNECT")
            .on_press_maybe((can_edit && app.network.is_some()).then_some(Message::Connect)),
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

fn room_view<'a>(app: &'a App, room: &'a RoomView) -> Element<'a, Message> {
    let guest = room
        .guest_name
        .as_deref()
        .unwrap_or("Waiting for opponent...");

    column![
        text(format!("Room #{}", room.id)).size(36),
        text(format!("Host: {}", room.host_name)),
        text(guest),
        button("LEAVE ROOM").on_press(Message::LeaveRoom),
        text(&app.status),
    ]
    .spacing(15)
    .padding(30)
    .into()
}

fn game_view<'a>(app: &'a App, game: &'a PlayerGameView) -> Element<'a, Message> {
    let round_text = match &app.last_round {
        None => "First battle".to_string(),

        Some(result) => {
            let outcome = match result.outcome {
                RoundOutcome::Win => format!("You win +{}!", result.points_awarded_to_you),

                RoundOutcome::Lose => "Opponent wins.".into(),

                RoundOutcome::Tie => "WAR! Play again.".into(),

                RoundOutcome::Split => "Final tie — pot split.".into(),
            };

            format!(
                "Last battle: {} vs {}. {}",
                result.your_card, result.opponent_card, outcome,
            )
        }
    };

    let turn_text = if game.finished {
        match game.your_score.cmp(&game.opponent_score) {
            Ordering::Greater => "Game over: You win!",
            Ordering::Less => "Game over: Your opponent wins.",
            Ordering::Equal => "Game over: Draw.",
        }
    } else if app.play_pending {
        "Playing your card..."
    } else if game.you_played {
        "Waiting for your opponent..."
    } else if game.opponent_played {
        "Your opponent played. It's your turn."
    } else {
        "Play a card."
    };

    let can_play = app.can_play(game);
    let play_button = button("PLAY CARD").on_press_maybe(can_play.then_some(Message::PlayCard));
    let table = Canvas::new(table::Table {
        art: &app.table_art,
        game,
        player_name: &app.username,
        last_round: app.last_round.as_ref(),
        play_pending: app.play_pending,
        can_play,
    })
    .width(iced::Fill)
    .height(iced::Fill);

    let content = column![
        row![
            text(format!("WAR / Room #{}", game.room_id)).size(24),
            space::horizontal(),
            button("LEAVE ROOM").on_press(Message::LeaveRoom),
        ]
        .align_y(iced::Center),
        row![
            text(format!(
                "{}: {} points",
                game.opponent_name, game.opponent_score
            )),
            space::horizontal(),
            text(format!("You: {} points", game.your_score)),
        ],
        table,
        row![
            play_button,
            column![text(turn_text).size(20), text(round_text).size(15)].spacing(4),
        ]
        .spacing(16)
        .align_y(iced::Center),
    ]
    .spacing(12)
    .padding(20);

    if app.status == "Connected" {
        content.into()
    } else {
        content.push(text(&app.status)).into()
    }
}

fn view(app: &App) -> Element<'_, Message> {
    if app.connection != Connection::Connected {
        return connect_view(app);
    }

    if let Some(game) = &app.game {
        return game_view(app, game);
    }

    if let Some(room) = &app.room {
        return room_view(app, room);
    }

    lobby_view(app)
}

fn main() -> iced::Result {
    iced::application(App::default, update, view)
        .subscription(subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1000.0, 760.0),
            min_size: Some(iced::Size::new(560.0, 480.0)),
            ..iced::window::Settings::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use war_protocol::{Card, Rank, Suit};

    pub(super) fn game() -> PlayerGameView {
        PlayerGameView {
            room_id: 1,
            opponent_name: "Guest".into(),
            your_score: 0,
            opponent_score: 0,
            your_cards_remaining: 26,
            opponent_cards_remaining: 26,
            battle_pot: 0,
            you_played: false,
            opponent_played: false,
            finished: false,
        }
    }

    #[test]
    fn guest_departure_returns_host_to_room_and_clears_the_game() {
        let card = Card {
            rank: Rank::Ace,
            suit: Suit::Spades,
        };
        let mut app = App {
            connection: Connection::Connected,
            game: Some(game()),
            last_round: Some(RoundResult {
                your_card: card,
                opponent_card: card,
                outcome: RoundOutcome::Tie,
                points_awarded_to_you: 0,
            }),
            play_pending: true,
            ..App::default()
        };

        handle_server_message(
            &mut app,
            ServerMessage::RoomState {
                room: RoomView {
                    id: 1,
                    host_name: "Host".into(),
                    guest_name: None,
                },
            },
        );

        assert!(app.room.is_some_and(|room| room.id == 1));
        assert!(app.game.is_none());
        assert!(app.last_round.is_none());
        assert!(!app.play_pending);
    }

    #[test]
    fn ignores_repeated_play_until_the_server_counts_the_card() {
        let (sender, mut commands) = mpsc::channel(10);
        let mut app = App {
            connection: Connection::Connected,
            network: Some(sender),
            game: Some(game()),
            ..App::default()
        };

        update(&mut app, Message::PlayCard);
        assert!(matches!(
            commands.try_recv(),
            Ok(network::Command::Send(ClientMessage::PlayCard))
        ));

        let mut opponent_played = game();
        opponent_played.opponent_played = true;
        opponent_played.opponent_cards_remaining -= 1;
        handle_server_message(
            &mut app,
            ServerMessage::GameState {
                game: opponent_played,
            },
        );
        update(&mut app, Message::PlayCard);
        assert!(matches!(
            commands.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        let mut resolved = game();
        resolved.your_cards_remaining -= 1;
        resolved.opponent_cards_remaining -= 1;
        handle_server_message(&mut app, ServerMessage::GameState { game: resolved });
        update(&mut app, Message::PlayCard);
        assert!(matches!(
            commands.try_recv(),
            Ok(network::Command::Send(ClientMessage::PlayCard))
        ));
    }
}
