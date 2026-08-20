use rand::seq::SliceRandom;

use war_protocol::{Card, ClientId, PlayerGameView, Rank, RoomId, RoundOutcome, RoundResult, Suit};

pub struct Game {
    pub room_id: RoomId,

    players: [ClientId; 2],
    names: [String; 2],

    decks: [Vec<Card>; 2],

    played: [Option<Card>; 2],

    scores: [u8; 2],

    battle_pot: u8,

    finished: bool,
}

pub struct Resolution {
    pub cards: [Card; 2],
    pub winner: Option<usize>,
    pub tie: bool,
    pub split: bool,
    pub points_awarded: u8,
}

fn create_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);

    for suit in Suit::ALL {
        for rank in Rank::ALL {
            deck.push(Card { rank, suit });
        }
    }

    deck
}

impl Game {
    pub fn new(
        room_id: RoomId,
        player1: ClientId,
        player1_name: String,
        player2: ClientId,
        player2_name: String,
    ) -> Self {
        let mut deck = create_deck();

        let mut rng = rand::rng();

        deck.shuffle(&mut rng);

        let player2_deck = deck.split_off(26);
        let player1_deck = deck;

        Self {
            room_id,

            players: [player1, player2],

            names: [player1_name, player2_name],

            decks: [player1_deck, player2_deck],

            played: [None, None],

            scores: [0, 0],

            battle_pot: 0,

            finished: false,
        }
    }
}

impl Game {
    fn player_index(&self, client_id: ClientId) -> Option<usize> {
        if self.players[0] == client_id {
            Some(0)
        } else if self.players[1] == client_id {
            Some(1)
        } else {
            None
        }
    }
}

impl Game {
    pub fn play_card(&mut self, client_id: ClientId) -> Result<Option<Resolution>, String> {
        if self.finished {
            return Err("The game is already finished.".into());
        }

        let player = self
            .player_index(client_id)
            .ok_or("You are not in this game.")?;

        if self.played[player].is_some() {
            return Err("You already played this battle.".into());
        }

        let card = self.decks[player].pop().ok_or("You have no cards left.")?;

        self.played[player] = Some(card);

        if self.played[0].is_none() || self.played[1].is_none() {
            return Ok(None);
        }

        Ok(Some(self.resolve_battle()))
    }
}

impl Game {
    fn resolve_battle(&mut self) -> Resolution {
        let card0 = self.played[0].unwrap();
        let card1 = self.played[1].unwrap();

        self.battle_pot += 2;

        let cards = [card0, card1];

        if card0.rank == card1.rank {
            return self.resolve_tie(cards);
        }

        let winner = if card0.rank > card1.rank { 0 } else { 1 };

        let points = self.battle_pot;

        self.scores[winner] += points;

        self.battle_pot = 0;
        self.played = [None, None];

        if self.decks[0].is_empty() && self.decks[1].is_empty() {
            self.finished = true;
        }

        Resolution {
            cards,
            winner: Some(winner),
            tie: false,
            split: false,
            points_awarded: points,
        }
    }
}

impl Game {
    fn resolve_tie(&mut self, cards: [Card; 2]) -> Resolution {
        self.played = [None, None];

        if self.decks[0].is_empty() && self.decks[1].is_empty() {
            let each = self.battle_pot / 2;

            self.scores[0] += each;
            self.scores[1] += each;

            self.battle_pot = 0;
            self.finished = true;

            return Resolution {
                cards,
                winner: None,
                tie: false,
                split: true,
                points_awarded: each,
            };
        }

        Resolution {
            cards,
            winner: None,
            tie: true,
            split: false,
            points_awarded: 0,
        }
    }
}

impl Game {
    pub fn view_for(&self, client_id: ClientId) -> Option<PlayerGameView> {
        let you = self.player_index(client_id)?;
        let opponent = 1 - you;

        Some(PlayerGameView {
            room_id: self.room_id,

            opponent_name: self.names[opponent].clone(),

            your_score: self.scores[you],

            opponent_score: self.scores[opponent],

            your_cards_remaining: self.decks[you].len() as u8,

            opponent_cards_remaining: self.decks[opponent].len() as u8,

            battle_pot: self.battle_pot,

            you_played: self.played[you].is_some(),

            opponent_played: self.played[opponent].is_some(),

            finished: self.finished,
        })
    }
}

impl Game {
    pub fn result_for(&self, resolution: &Resolution, client_id: ClientId) -> Option<RoundResult> {
        let you = self.player_index(client_id)?;
        let opponent = 1 - you;

        let outcome = if resolution.split {
            RoundOutcome::Split
        } else if resolution.tie {
            RoundOutcome::Tie
        } else if resolution.winner == Some(you) {
            RoundOutcome::Win
        } else {
            RoundOutcome::Lose
        };

        let points_awarded_to_you = if resolution.split {
            resolution.points_awarded
        } else if resolution.winner == Some(you) {
            resolution.points_awarded
        } else {
            0
        };

        Some(RoundResult {
            your_card: resolution.cards[you],

            opponent_card: resolution.cards[opponent],

            outcome,

            points_awarded_to_you,
        })
    }
}
