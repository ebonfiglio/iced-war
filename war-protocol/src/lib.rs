use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_SIZE: usize = 64 * 1024;

pub type ClientId = u64;
pub type RoomId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rank {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

impl Rank {
    pub const ALL: [Rank; 13] = [
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
        Rank::Ace,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rank = match self.rank {
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
        };

        let suit = match self.suit {
            Suit::Clubs => "♣",
            Suit::Diamonds => "♦",
            Suit::Hearts => "♥",
            Suit::Spades => "♠",
        };

        write!(f, "{rank}{suit}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSummary {
    pub id: RoomId,
    pub host_name: String,
    pub player_count: u8,
    pub in_game: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomView {
    pub id: RoomId,
    pub host_name: String,
    pub guest_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerGameView {
    pub room_id: RoomId,

    pub opponent_name: String,

    pub your_score: u8,
    pub opponent_score: u8,

    pub your_cards_remaining: u8,
    pub opponent_cards_remaining: u8,

    pub battle_pot: u8,

    pub you_played: bool,
    pub opponent_played: bool,

    pub finished: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RoundOutcome {
    Win,
    Lose,
    Tie,
    Split,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundResult {
    pub your_card: Card,
    pub opponent_card: Card,

    pub outcome: RoundOutcome,

    pub points_awarded_to_you: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello {
        protocol_version: u32,
        username: String,
    },

    RequestRooms,

    CreateRoom,

    JoinRoom {
        room_id: RoomId,
    },

    LeaveRoom,

    PlayCard,

    Ping {
        nonce: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome { client_id: ClientId },

    RoomList { rooms: Vec<RoomSummary> },

    RoomState { room: RoomView },

    GameState { game: PlayerGameView },

    RoundResult { result: RoundResult },

    LeftRoom,

    Error { message: String },

    Pong { nonce: u64 },
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, bincode::error::EncodeError> {
    bincode::serde::encode_to_vec(value, bincode::config::standard())
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, bincode::error::DecodeError> {
    let (value, _bytes_read) =
        bincode::serde::decode_from_slice(bytes, bincode::config::standard())?;

    Ok(value)
}
